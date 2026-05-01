use crate::application::actors::system::{Actor, ActorRef, Command, Message};
use crate::application::actors::EventRouter;
use crate::application::errors::Result;
use crate::application::events::{AIAdvisorEvent, Event, StrategyEvent};
use crate::core::domain::trading::Signal;
use crate::infrastructure::ai::ClaudeAdvisor;
use async_trait::async_trait;
use log::{debug, info, warn};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

pub struct AIAdvisorActor {
    advisor: Option<ClaudeAdvisor>,
    event_router: Arc<EventRouter>,
    sender: mpsc::UnboundedSender<Message>,
    receiver: Option<mpsc::UnboundedReceiver<Message>>,
    running: Arc<RwLock<bool>>,
    open_positions: Arc<RwLock<usize>>,
    max_positions: usize,
    daily_pnl_pct: Arc<RwLock<f64>>,
}

impl AIAdvisorActor {
    pub fn new(
        api_key: Option<String>,
        event_router: Arc<EventRouter>,
        max_positions: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let advisor = api_key.map(ClaudeAdvisor::new);

        if advisor.is_none() {
            warn!(
                "AIAdvisorActor: no Anthropic API key — signals pass through without AI analysis"
            );
        } else {
            info!("AIAdvisorActor: Claude AI advisor enabled (claude-haiku)");
        }

        Self {
            advisor,
            event_router,
            sender,
            receiver: Some(receiver),
            running: Arc::new(RwLock::new(false)),
            open_positions: Arc::new(RwLock::new(0)),
            max_positions,
            daily_pnl_pct: Arc::new(RwLock::new(0.0)),
        }
    }

    pub fn actor_ref(&self) -> ActorRef {
        self.sender.clone()
    }
}

#[async_trait]
impl Actor for AIAdvisorActor {
    fn name(&self) -> &str {
        "AIAdvisorActor"
    }

    fn is_running(&self) -> bool {
        self.running.try_read().map(|r| *r).unwrap_or(false)
    }

    async fn start(&mut self) -> Result<()> {
        *self.running.write().await = true;
        let mut receiver = self.receiver.take().ok_or_else(|| {
            crate::application::errors::Error::Internal(
                "AIAdvisorActor already started".to_string(),
            )
        })?;

        let running = self.running.clone();
        let event_router = self.event_router.clone();
        let advisor = self.advisor.clone();
        let open_positions = self.open_positions.clone();
        let max_positions = self.max_positions;
        let daily_pnl_pct = self.daily_pnl_pct.clone();

        tokio::spawn(async move {
            while *running.read().await {
                let msg = tokio::select! {
                    msg = receiver.recv() => msg,
                    else => break,
                };

                match msg {
                    Some(Message::Event(event)) => {
                        if let Event::Strategy(StrategyEvent::Signal {
                            token_id,
                            signal,
                            metadata,
                            timestamp: _,
                        }) = *event
                        {
                            if signal == Signal::Buy {
                                let open = *open_positions.read().await;
                                let daily_pnl = *daily_pnl_pct.read().await;

                                let (approved, confidence, reasoning) = match &advisor {
                                    None => {
                                        debug!("AIAdvisor: no advisor, passing through");
                                        (true, 75u8, "AI advisor not configured".to_string())
                                    }
                                    Some(adv) => {
                                        info!(
                                            "🤖 AIAdvisor analysing BUY for {} (RSI={:.1})",
                                            &token_id[..token_id.len().min(10)],
                                            metadata.rsi.unwrap_or(50.0)
                                        );
                                        match adv
                                            .analyse_signal(
                                                &token_id[..token_id.len().min(10)],
                                                &token_id,
                                                metadata.rsi.unwrap_or(50.0),
                                                metadata.bollinger_pct.unwrap_or(50.0),
                                                metadata.volume_24h.unwrap_or(0.0),
                                                metadata.price_change_24h.unwrap_or(0.0),
                                                metadata.signal_price,
                                                metadata.momentum_score.unwrap_or(0.0),
                                                open,
                                                max_positions,
                                                daily_pnl,
                                            )
                                            .await
                                        {
                                            Ok(d) => {
                                                if d.approve {
                                                    info!(
                                                        "✅ AI APPROVED {} ({}%) — {}",
                                                        &token_id[..token_id.len().min(10)],
                                                        d.confidence,
                                                        d.reasoning
                                                    );
                                                } else {
                                                    info!(
                                                        "❌ AI REJECTED {} ({}%) — {}",
                                                        &token_id[..token_id.len().min(10)],
                                                        d.confidence,
                                                        d.reasoning
                                                    );
                                                }
                                                (d.approve, d.confidence, d.reasoning)
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "AIAdvisor: Claude error ({}), failing open",
                                                    e
                                                );
                                                (true, 50, format!("AI unavailable: {}", e))
                                            }
                                        }
                                    }
                                };

                                // Publish AIAdvisorEvent — routes to risk (if approved) and database.
                                // Approved signals are handled by RiskManager via AIAdvisorEvent;
                                // re-emitting as Strategy would loop back to this actor.
                                if let Err(e) = event_router
                                    .publish(Event::AIAdvisor(AIAdvisorEvent::SignalAnalysed {
                                        token_id,
                                        signal,
                                        approved,
                                        confidence,
                                        reasoning,
                                        metadata,
                                    }))
                                    .await
                                {
                                    warn!(
                                        "AIAdvisor: failed to publish SignalAnalysed event: {}",
                                        e
                                    );
                                }
                            } else {
                                // SELL/HOLD — bypass AI, forward directly as AIAdvisor approved
                                // (routes to risk via AIAdvisor → risk routing, no loop)
                                if let Err(e) = event_router
                                    .publish(Event::AIAdvisor(AIAdvisorEvent::SignalAnalysed {
                                        token_id,
                                        signal,
                                        approved: true,
                                        confidence: 100,
                                        reasoning: "SELL signal — bypasses AI advisor".to_string(),
                                        metadata,
                                    }))
                                    .await
                                {
                                    warn!(
                                        "AIAdvisor: failed to publish SignalAnalysed event: {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Some(Message::Command(Command::Stop)) | None => break,
                    _ => {}
                }
            }
            info!("AIAdvisorActor stopped");
        });

        info!("AIAdvisorActor started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        *self.running.write().await = false;
        Ok(())
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        let _ = self.sender.send(Message::Event(Box::new(event)));
        Ok(())
    }
}
