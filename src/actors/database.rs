use super::{Actor, Message, Event, DatabaseEvent, Command, Query, QueryResult, MarketEvent, ExecutionEvent};
use crate::error::Error;
use crate::repositories::TokenRepository;
use crate::types::market::TokenMetrics;
use std::sync::Arc;
// use tokio::sync::RwLock; // Unused import
use chrono::Utc;
use log::{info, error, debug}; // debug is unused
use tokio;

pub struct DatabaseActor {
    token_repo: TokenRepository,
    message_bus: Arc<super::MessageBus>,
    running: bool,
}

impl DatabaseActor {
    pub fn new(
        token_repo: TokenRepository,
        message_bus: Arc<super::MessageBus>,
    ) -> Self {
        Self {
            token_repo,
            message_bus,
            running: false,
        }
    }

    async fn handle_token_update(&mut self, token_id: String, metrics: TokenMetrics) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }

        // Update token metadata
        self.token_repo.update_token_metadata(&token_id, &metrics.symbol)?;
        
        // Store price data point 
        self.token_repo.get_db().store_price_data(&token_id, metrics.price_usd, metrics.volume_24h)?;

        // Publish database event
        let event = Event::Database(DatabaseEvent::TokenUpdated {
            token_id,
            timestamp: Utc::now(),
        });

        self.message_bus.publish(event).await?;
        Ok(())
    }

    async fn handle_trade_execution(&mut self, token_id: String, price: f64, size: f64, is_buy: bool) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }

        // Store trade execution
        self.token_repo.store_trade_execution(
            &token_id,
            price,
            size,
            is_buy,
            Utc::now(),
        )?;

        // Publish database event
        let event = Event::Database(DatabaseEvent::TradeExecuted {
            token_id,
            price,
            size,
            is_buy,
            timestamp: Utc::now(),
        });

        self.message_bus.publish(event).await?;
        Ok(())
    }

    async fn handle_position_update(&mut self, token_id: String, price: f64, pnl: f64) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }

        // Store position update
        self.token_repo.store_position_update(
            &token_id,
            price,
            pnl,
            Utc::now(),
        )?;

        // Publish database event
        let event = Event::Database(DatabaseEvent::PositionUpdated {
            token_id,
            price,
            pnl,
            timestamp: Utc::now(),
        });

        self.message_bus.publish(event).await?;
        Ok(())
    }
}

impl Actor for DatabaseActor {
    fn start(&mut self) -> Result<(), Error> {
        self.running = true;
        info!("Starting DatabaseActor");
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("Stopping DatabaseActor");
        Ok(())
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::Command(Command::Start) => self.start(),
            Message::Command(Command::Stop) => self.stop(),
            Message::Command(Command::UpdateConfig(_)) => Ok(()),
            Message::Event(event) => {
                match event {
                    Event::Market(MarketEvent::PriceUpdate { token_id, price, volume, .. }) => {
                        if !self.running {
                            return Ok(());
                        }
                        
                        // Just store the price data
                        if let Err(e) = tokio::task::block_in_place(|| {
                            self.token_repo.get_db().store_price_data(&token_id, price, volume.unwrap_or(0.0))
                        }) {
                            error!("Error storing price data: {}", e);
                        }
                        Ok(())
                    },
                    Event::Execution(ExecutionEvent::OrderExecuted { token_id, signal, size, price, .. }) => {
                        if !self.running {
                            return Ok(());
                        }
                        
                        // Determine if this is a buy order based on the signal
                        let is_buy = signal == crate::trading::strategy::Signal::Buy;
                        
                        info!("DatabaseActor received OrderExecuted event for {}: is_buy={}, price=${:.4}, size=${:.2}", 
                            token_id, is_buy, price, size);
                        
                        // Clone what we need for the async task
                        let token_repo = self.token_repo.clone();
                        let message_bus = self.message_bus.clone();
                        let token_id_clone = token_id.clone();
                        
                        tokio::spawn(async move {
                            if let Err(e) = tokio::task::block_in_place(|| {
                                // Store trade execution
                                token_repo.store_trade_execution(
                                    &token_id_clone,
                                    price,
                                    size,
                                    is_buy,
                                    Utc::now(),
                                )
                            }) {
                                error!("Error handling trade execution: {}", e);
                                return;
                            }
                            
                            // Publish database event
                            let event = Event::Database(DatabaseEvent::TradeExecuted {
                                token_id: token_id_clone,
                                price,
                                size,
                                is_buy,
                                timestamp: Utc::now(),
                            });
                            
                            if let Err(e) = message_bus.publish(event).await {
                                error!("Error publishing trade execution event: {}", e);
                            }
                        });
                        
                        Ok(())
                    },
                    Event::Execution(ExecutionEvent::PositionUpdate { token_id, current_price, pnl, .. }) => {
                        if !self.running {
                            return Ok(());
                        }
                        
                        // Clone what we need for the async task
                        let token_repo = self.token_repo.clone();
                        let message_bus = self.message_bus.clone();
                        let token_id_clone = token_id.clone();
                        
                        tokio::spawn(async move {
                            if let Err(e) = tokio::task::block_in_place(|| {
                                // Store position update
                                token_repo.store_position_update(
                                    &token_id_clone,
                                    current_price,
                                    pnl,
                                    Utc::now(),
                                )
                            }) {
                                error!("Error handling position update: {}", e);
                                return;
                            }
                            
                            // Publish database event
                            let event = Event::Database(DatabaseEvent::PositionUpdated {
                                token_id: token_id_clone,
                                price: current_price,
                                pnl,
                                timestamp: Utc::now(),
                            });
                            
                            if let Err(e) = message_bus.publish(event).await {
                                error!("Error publishing position update event: {}", e);
                            }
                        });
                        
                        Ok(())
                    },
                    _ => Ok(()),
                }
            },
            Message::Query(query, responder) => {
                match query {
                    Query::GetStatus => {
                        let status = format!("DatabaseActor running: {}", self.running);
                        let _ = responder.send(Ok(QueryResult::Status(status)));
                        Ok(())
                    },
                    Query::GetMetrics => {
                        // Get database metrics
                        let metrics = serde_json::json!({
                            "running": self.running,
                            "records": {
                                "tokens": self.token_repo.get_token_count().unwrap_or(0),
                                "trades": self.token_repo.get_trade_count().unwrap_or(0)
                            }
                        });
                        let _ = responder.send(Ok(QueryResult::Metrics(metrics)));
                        Ok(())
                    },
                    _ => {
                        let _ = responder.send(Err(Error::InvalidInput("Unsupported query type for DatabaseActor".to_string())));
                        Ok(())
                    }
                }
            }
        }
    }
} 