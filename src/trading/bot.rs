use crate::error::Error;
// TokenMetrics isn't used in this file
// use crate::types::market::TokenMetrics;
use crate::trading::strategy::{Strategy, Position};
// Signal is only used as part of imported types, not directly
// use crate::trading::strategy::{Strategy, Signal, Position};
use crate::dex::DexClient;
use crate::repositories::RepositoryFactory;
use crate::api::market::MarketApi;
use crate::actors::{
    MessageBus, MarketDataActor, StrategyActor, 
    RiskManagerActor, ExecutionActor, DatabaseActor, SupervisorActor,
    spawn_actor, ActorRef, Actor
};
use crate::config;
use crate::db::Database;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
// RwLock isn't used in this file
// use tokio::sync::{mpsc, RwLock};
// Only info and error are used, debug and warn are not
use log::{info, error, warn, debug, trace};
// use log::{info, error, debug, warn};
use chrono::Utc;
use serde_json::json;
use futures::pin_mut;
use tokio::time::timeout;

// Global shutdown flag to ensure all tasks exit
static FORCE_SHUTDOWN: AtomicBool = AtomicBool::new(false);

// Check if we're in forced shutdown mode
pub fn is_forced_shutdown() -> bool {
    FORCE_SHUTDOWN.load(Ordering::SeqCst)
}

// Set force shutdown flag to ensure all tasks exit
pub fn set_forced_shutdown() {
    FORCE_SHUTDOWN.store(true, Ordering::SeqCst);
}

/// TradingBotSystem manages the actor-based trading system
pub struct TradingBotSystem {
    /// Has the bot been started?
    running: bool,
    /// The message bus for actor communication
    message_bus: Arc<MessageBus>,
    /// Actor references
    supervisor: Option<SupervisorActor>,
    market_actor_ref: Option<crate::actors::ActorRef>,
    strategy_actor_ref: Option<crate::actors::ActorRef>,
    risk_actor_ref: Option<crate::actors::ActorRef>,
    execution_actor_ref: Option<crate::actors::ActorRef>,
    database_actor_ref: Option<crate::actors::ActorRef>,
    /// Sender channels
    market_sender: Option<mpsc::Sender<crate::actors::Message>>,
    strategy_sender: Option<mpsc::Sender<crate::actors::Message>>,
    risk_sender: Option<mpsc::Sender<crate::actors::Message>>,
    execution_sender: Option<mpsc::Sender<crate::actors::Message>>,
    database_sender: Option<mpsc::Sender<crate::actors::Message>>,
    /// Configuration
    config: config::Config,
    /// Database path
    db_path: String,
}

impl TradingBotSystem {
    /// Create a new trading bot system
    pub fn new(config: config::Config, db_path: String) -> Self {
        // Get the singleton MessageBus instance instead of creating a new one
        let message_bus = MessageBus::instance();
        
        // Get unique pointer address of MessageBus for logging
        let bus_id = format!("{:p}", Arc::as_ptr(&message_bus));
        info!("🔄 Creating TradingBotSystem with global MessageBus instance [id: {}]", bus_id);
        debug!("This ensures all components share the same MessageBus instance for proper event routing");
        
        Self {
            running: false,
            message_bus,
            supervisor: None,
            market_actor_ref: None,
            strategy_actor_ref: None,
            risk_actor_ref: None,
            execution_actor_ref: None,
            database_actor_ref: None,
            market_sender: None,
            strategy_sender: None,
            risk_sender: None,
            execution_sender: None,
            database_sender: None,
            config,
            db_path,
        }
    }
    
    /// Create a new trading bot system with an existing MessageBus
    pub fn with_message_bus(config: config::Config, db_path: String, message_bus: Arc<MessageBus>) -> Self {
        // Get unique pointer address of MessageBus for logging
        let bus_id = format!("{:p}", Arc::as_ptr(&message_bus));
        info!("🔄 Creating TradingBotSystem with existing MessageBus instance [id: {}]", bus_id);
        debug!("TradingBotSystem using MessageBus [{}] with {} subscribers", 
               bus_id, "to be queried"); // We can't access subscribers directly here
        
        Self {
            running: false,
            message_bus,
            supervisor: None,
            market_actor_ref: None,
            strategy_actor_ref: None,
            risk_actor_ref: None,
            execution_actor_ref: None,
            database_actor_ref: None,
            market_sender: None,
            strategy_sender: None,
            risk_sender: None,
            execution_sender: None,
            database_sender: None,
            config,
            db_path,
        }
    }
    
    /// Start the trading bot system
    pub async fn start(&mut self, strategy_type: &str, params: &serde_json::Value) -> Result<(), Error> {
        if self.running {
            return Err(Error::InvalidInput("Trading bot is already running".to_string()));
        }
        
        // Get unique pointer address of MessageBus for logging
        let bus_id = format!("{:p}", Arc::as_ptr(&self.message_bus));
        info!("🚀 Starting actor-based trading bot system with MessageBus [id: {}]", bus_id);
        
        // Update risk tolerance level from params if provided
        if let Some(risk_level) = params.get("risk_tolerance").and_then(|v| v.as_u64()).map(|v| v as u8) {
            info!("Setting risk tolerance level to {}", risk_level);
            self.config.trading.risk_tolerance = risk_level;
        }
        
        // Create debug subscriber for market events to ensure we have at least one subscriber
        // This helps prevent messages from being dropped when no one is listening
        let (debug_market_sender, mut debug_market_receiver) = mpsc::channel::<crate::actors::Event>(100);
        info!("🛠️ Creating diagnostic market event subscriber to prevent message drops");
        self.message_bus.subscribe("market".to_string(), debug_market_sender).await?;
        
        // Double-check the subscription was successful
        let market_subs = self.message_bus.debug_subscriber_count("market").await;
        if market_subs > 0 {
            info!("✅ Verified market event subscriber is registered (count: {})", market_subs);
        } else {
            error!("❌ Failed to register market event subscriber! Messages will be dropped.");
        }
        
        // Spawn a task to monitor and log market events for diagnostic purposes
        let debug_message_bus = self.message_bus.clone();
        tokio::spawn(async move {
            info!("📡 Market event diagnostic monitor started");
            let mut event_count = 0;
            
            while let Some(event) = debug_market_receiver.recv().await {
                event_count += 1;
                if event_count % 10 == 0 {  // Only log every 10 events to avoid spamming
                    debug!("📊 Debug market event monitor received event #{}", event_count);
                    
                    // Check if we still have subscribers for sanity
                    let count = debug_message_bus.debug_subscriber_count("market").await;
                    debug!("Current market subscriber count: {}", count);
                }
            }
            
            warn!("⚠️ Market event diagnostic monitor terminated after {} events", event_count);
        });
        
        // Log subscriber count for each event type to help with debugging
        let market_subs = self.message_bus.debug_subscriber_count("market").await;
        let strategy_subs = self.message_bus.debug_subscriber_count("strategy").await;
        let risk_subs = self.message_bus.debug_subscriber_count("risk").await;
        let execution_subs = self.message_bus.debug_subscriber_count("execution").await;
        let database_subs = self.message_bus.debug_subscriber_count("database").await;
        
        info!("MessageBus [{}] subscribers before start - market: {}, strategy: {}, risk: {}, execution: {}, database: {}", 
              bus_id, market_subs, strategy_subs, risk_subs, execution_subs, database_subs);
        
        info!("Starting actor-based trading bot system");
        
        // Create repositories and clients
        let mut config = self.config.clone();
        config.database.custom_path = Some(std::path::PathBuf::from(&self.db_path));
        
        // Create a database using our path
        let db = Database::new()
            .map_err(|e| Error::from(e))?;
        
        // Create repository factory with the database
        let repo_factory = RepositoryFactory::with_db(db, self.config.trading.paper_trading);
        
        // Get token repository from the factory
        let token_repo = repo_factory.token_repository();
        
        // Initialize market API based on config
        let market_api = self.create_market_api()?;
        
        // Create the strategy
        let strategy = self.create_strategy(strategy_type, params)?;
        
        // Create DEX client
        let dex_client = if self.config.trading.paper_trading {
            info!("Running in paper trading mode");
            DexClient::new_paper_trading()
        } else {
            info!("Running in LIVE trading mode - REAL trades will be executed!");
            DexClient::new_live()
        };
        
        // Create tokens to track based on the config
        let tokens_to_track = self.get_tokens_to_track();
        
        // Create supervisor
        let supervisor = SupervisorActor::new(self.message_bus.clone());
        self.supervisor = Some(supervisor);
        
        // Create channels for each actor
        let (market_sender, mut market_receiver) = mpsc::channel(100);
        let (strategy_sender, mut strategy_receiver) = mpsc::channel(100);
        let (risk_sender, mut risk_receiver) = mpsc::channel(100);
        let (execution_sender, mut execution_receiver) = mpsc::channel(100);
        let (database_sender, mut database_receiver) = mpsc::channel(100);
        
        // Save the sender channels right away
        self.market_sender = Some(market_sender.clone());
        self.strategy_sender = Some(strategy_sender.clone());
        self.risk_sender = Some(risk_sender.clone());
        self.execution_sender = Some(execution_sender.clone());
        self.database_sender = Some(database_sender.clone());
        
        // Create ActorRefs for direct communication
        self.market_actor_ref = Some(ActorRef::new("market".to_string(), market_sender.clone()));
        self.strategy_actor_ref = Some(ActorRef::new("strategy".to_string(), strategy_sender.clone()));
        self.risk_actor_ref = Some(ActorRef::new("risk".to_string(), risk_sender.clone()));
        self.execution_actor_ref = Some(ActorRef::new("execution".to_string(), execution_sender.clone()));
        self.database_actor_ref = Some(ActorRef::new("database".to_string(), database_sender.clone()));
        
        // Get the appropriate market data provider based on config
        let provider = market_api.get_configured_provider(&self.config);
        info!("Using {} as market data provider", provider.name());
        let supports_websocket = provider.supports_websocket();
        
        // Create actors
        let market_actor = if supports_websocket {
            info!("👉 Using WebSocket for real-time price data");
            MarketDataActor::with_websocket(
                market_api,
                token_repo.clone(),
                self.message_bus.clone(),
                tokens_to_track,
            )
        } else {
            info!("👉 Using polling for price data (WebSocket not supported)");
            MarketDataActor::new(
                market_api,
                token_repo.clone(),
                self.message_bus.clone(),
                self.config.trading.scan_interval,
            )
        };
        
        let strategy_actor = StrategyActor::new(
            strategy,
            token_repo.clone(),
            self.message_bus.clone(),
        );
        
        let risk_actor = RiskManagerActor::new(
            token_repo.clone(),
            self.message_bus.clone(),
            self.config.trading.max_position_size,
            self.config.trading.risk.stop_loss_pct,
            self.config.trading.risk.take_profit_pct,
        );
        
        let execution_actor = ExecutionActor::new(
            token_repo.clone(),
            dex_client,
            self.message_bus.clone(),
            self.config.clone(),
        );
        
        let database_actor = DatabaseActor::new(
            token_repo.clone(),
            self.message_bus.clone(),
        );
        
        // Spawn actor tasks directly
        let market_id = "market".to_string();
        tokio::spawn(async move {
            info!("Actor task started for {}", market_id);
            let mut market_actor = market_actor;
            while let Some(msg) = market_receiver.recv().await {
                // Check for forced shutdown signal
                if is_forced_shutdown() {
                    info!("Force shutdown detected, terminating market actor task");
                    break;
                }
                
                if let Err(e) = market_actor.handle_message(msg) {
                    error!("Error handling message in market actor: {}", e);
                }
            }
            warn!("Market actor task has terminated - channel closed");
        });
        
        let strategy_id = "strategy".to_string();
        tokio::spawn(async move {
            info!("Actor task started for {}", strategy_id);
            let mut strategy_actor = strategy_actor;
            while let Some(msg) = strategy_receiver.recv().await {
                // Check for forced shutdown signal
                if is_forced_shutdown() {
                    info!("Force shutdown detected, terminating strategy actor task");
                    break;
                }
                
                if let Err(e) = strategy_actor.handle_message(msg) {
                    error!("Error handling message in strategy actor: {}", e);
                }
            }
            warn!("Strategy actor task has terminated - channel closed");
        });
        
        let risk_id = "risk".to_string();
        tokio::spawn(async move {
            info!("Actor task started for {}", risk_id);
            let mut risk_actor = risk_actor;
            while let Some(msg) = risk_receiver.recv().await {
                // Check for forced shutdown signal
                if is_forced_shutdown() {
                    info!("Force shutdown detected, terminating risk actor task");
                    break;
                }
                
                if let Err(e) = risk_actor.handle_message(msg) {
                    error!("Error handling message in risk actor: {}", e);
                }
            }
            warn!("Risk actor task has terminated - channel closed");
        });
        
        let execution_id = "execution".to_string();
        tokio::spawn(async move {
            info!("Actor task started for {}", execution_id);
            let mut execution_actor = execution_actor;
            while let Some(msg) = execution_receiver.recv().await {
                // Check for forced shutdown signal
                if is_forced_shutdown() {
                    info!("Force shutdown detected, terminating execution actor task");
                    break;
                }
                
                if let Err(e) = execution_actor.handle_message(msg) {
                    error!("Error handling message in execution actor: {}", e);
                }
            }
            warn!("Execution actor task has terminated - channel closed");
        });
        
        let database_id = "database".to_string();
        tokio::spawn(async move {
            info!("Actor task started for {}", database_id);
            let mut database_actor = database_actor;
            while let Some(msg) = database_receiver.recv().await {
                // Check for forced shutdown signal
                if is_forced_shutdown() {
                    info!("Force shutdown detected, terminating database actor task");
                    break;
                }
                
                if let Err(e) = database_actor.handle_message(msg) {
                    error!("Error handling message in database actor: {}", e);
                }
            }
            warn!("Database actor task has terminated - channel closed");
        });
        
        // Register actors with supervisor
        if let Some(supervisor) = &self.supervisor {
            if let Some(market_ref) = &self.market_actor_ref {
                supervisor.register_actor("market".to_string(), market_ref.clone()).await?;
            }
            if let Some(strategy_ref) = &self.strategy_actor_ref {
                supervisor.register_actor("strategy".to_string(), strategy_ref.clone()).await?;
            }
            if let Some(risk_ref) = &self.risk_actor_ref {
                supervisor.register_actor("risk".to_string(), risk_ref.clone()).await?;
            }
            if let Some(execution_ref) = &self.execution_actor_ref {
                supervisor.register_actor("execution".to_string(), execution_ref.clone()).await?;
            }
            if let Some(database_ref) = &self.database_actor_ref {
                supervisor.register_actor("database".to_string(), database_ref.clone()).await?;
            }
        }
        
        // Create proper event channels and set up forwarding to the actor message channels
        let market_sender_clone = self.market_sender.clone().unwrap();
        let (market_event_sender, mut market_event_receiver) = mpsc::channel::<crate::actors::Event>(100);
        let market_event_sender_for_strategy = market_event_sender.clone();
        let market_event_sender_for_db = market_event_sender.clone();
        tokio::spawn(async move {
            while let Some(event) = market_event_receiver.recv().await {
                // Check for forced shutdown
                if is_forced_shutdown() {
                    debug!("Force shutdown detected, terminating market event forwarder");
                    break;
                }
                
                if let Err(e) = market_sender_clone.send(crate::actors::Message::Event(event)).await {
                    error!("Failed to forward market event: {}", e);
                }
            }
        });
        
        let strategy_sender_clone = self.strategy_sender.clone().unwrap();
        let (strategy_event_sender, mut strategy_event_receiver) = mpsc::channel::<crate::actors::Event>(100);
        let strategy_event_sender_for_risk = strategy_event_sender.clone();
        let strategy_event_sender_for_db = strategy_event_sender.clone();
        tokio::spawn(async move {
            while let Some(event) = strategy_event_receiver.recv().await {
                // Check for forced shutdown
                if is_forced_shutdown() {
                    debug!("Force shutdown detected, terminating strategy event forwarder");
                    break;
                }
                
                if let Err(e) = strategy_sender_clone.send(crate::actors::Message::Event(event)).await {
                    error!("Failed to forward strategy event: {}", e);
                }
            }
        });
        
        let risk_sender_clone = self.risk_sender.clone().unwrap();
        let (risk_event_sender, mut risk_event_receiver) = mpsc::channel::<crate::actors::Event>(100);
        let risk_event_sender_for_exec = risk_event_sender.clone();
        let risk_event_sender_for_db = risk_event_sender.clone();
        tokio::spawn(async move {
            while let Some(event) = risk_event_receiver.recv().await {
                // Check for forced shutdown
                if is_forced_shutdown() {
                    debug!("Force shutdown detected, terminating risk event forwarder");
                    break;
                }
                
                if let Err(e) = risk_sender_clone.send(crate::actors::Message::Event(event)).await {
                    error!("Failed to forward risk event: {}", e);
                }
            }
        });
        
        let execution_sender_clone = self.execution_sender.clone().unwrap();
        let (execution_event_sender, mut execution_event_receiver) = mpsc::channel::<crate::actors::Event>(100);
        let execution_event_sender_for_db = execution_event_sender.clone();
        tokio::spawn(async move {
            while let Some(event) = execution_event_receiver.recv().await {
                // Check for forced shutdown
                if is_forced_shutdown() {
                    debug!("Force shutdown detected, terminating execution event forwarder");
                    break;
                }
                
                if let Err(e) = execution_sender_clone.send(crate::actors::Message::Event(event)).await {
                    error!("Failed to forward execution event: {}", e);
                }
            }
        });
        
        let database_sender_clone = self.database_sender.clone().unwrap();
        let (database_event_sender, mut database_event_receiver) = mpsc::channel::<crate::actors::Event>(100);
        tokio::spawn(async move {
            while let Some(event) = database_event_receiver.recv().await {
                // Check for forced shutdown
                if is_forced_shutdown() {
                    debug!("Force shutdown detected, terminating database event forwarder");
                    break;
                }
                
                if let Err(e) = database_sender_clone.send(crate::actors::Message::Event(event)).await {
                    error!("Failed to forward database event: {}", e);
                }
            }
        });
        
        // Subscribe event channels to the message bus
        info!("🔄 Subscribing actor channels to message bus events...");
        self.message_bus.subscribe("market".to_string(), market_event_sender).await?;
        debug!("✅ Subscribed market events to primary handler");
        self.message_bus.subscribe("strategy".to_string(), strategy_event_sender).await?;
        debug!("✅ Subscribed strategy events to primary handler");
        self.message_bus.subscribe("risk".to_string(), risk_event_sender).await?;
        debug!("✅ Subscribed risk events to primary handler");
        self.message_bus.subscribe("execution".to_string(), execution_event_sender).await?;
        debug!("✅ Subscribed execution events to primary handler");
        self.message_bus.subscribe("database".to_string(), database_event_sender.clone()).await?;
        debug!("✅ Subscribed database events to primary handler");
        
        // Create a special channel just for the strategy actor to receive market events
        let (strategy_market_sender, mut strategy_market_receiver) = mpsc::channel::<crate::actors::Event>(500);
        
        // Subscribe this sender to market events
        info!("🔌 Setting up market event subscription for StrategyActor");
        debug!("Creating market event subscription: MessageBus → StrategyActor");
        self.message_bus.subscribe("market".to_string(), strategy_market_sender).await?;
        debug!("✅ Subscribed market events to strategy actor");
        
        // Verify the subscription was successful
        let market_subs = self.message_bus.debug_subscriber_count("market").await;
        info!("Market subscriber count after Strategy subscription: {}", market_subs);
        
        // Create a dedicated task for the Strategy actor to process market events
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            let strategy_ref = strategy_ref.clone();
            tokio::spawn(async move {
                info!("🔄 Market → Strategy event forwarder started");
                let mut event_count = 0;
                
                while let Some(event) = strategy_market_receiver.recv().await {
                    // Check for forced shutdown
                    if is_forced_shutdown() {
                        info!("Force shutdown detected, terminating Market → Strategy event forwarder");
                        break;
                    }
                    
                    event_count += 1;
                    if let crate::actors::Event::Market(market_event) = event.clone() {
                        // Forward the market event to the strategy actor
                        if let Err(e) = strategy_ref.send(crate::actors::Message::Event(event)).await {
                            error!("Failed to forward market event to strategy actor: {}", e);
                        } else if event_count % 10 == 0 {
                            debug!("Forwarded {} market events to strategy actor", event_count);
                        }
                    }
                }
                
                warn!("Market → Strategy event forwarder terminated after {} events", event_count);
            });
        }
        
        // Key event flows - subscribe actors to receive events they need to process
        
        // 1. Market → Strategy (market data flows to strategy decisions)
        // Note: The Market → Strategy subscription is already set up above with a dedicated task
        // that forwards events from the MessageBus to the StrategyActor
        
        // 2. Strategy → Risk (signals flow to risk assessment)
        let (risk_strategy_sender, mut risk_strategy_receiver) = mpsc::channel::<crate::actors::Event>(100);
        info!("Creating subscription: Strategy events → Risk actor");
        self.message_bus.subscribe("strategy".to_string(), risk_strategy_sender).await?;
        
        // Create a dedicated task for the Risk actor to process strategy events
        if let Some(ref risk_ref) = self.risk_actor_ref {
            let risk_ref = risk_ref.clone();
            tokio::spawn(async move {
                info!("🔄 Strategy → Risk event forwarder started");
                let mut event_count = 0;
                
                while let Some(event) = risk_strategy_receiver.recv().await {
                    // Check for forced shutdown
                    if is_forced_shutdown() {
                        info!("Force shutdown detected, terminating Strategy → Risk event forwarder");
                        break;
                    }
                    
                    event_count += 1;
                    if let crate::actors::Event::Strategy(strategy_event) = event.clone() {
                        // Forward the strategy event to the risk actor
                        if let Err(e) = risk_ref.send(crate::actors::Message::Event(event)).await {
                            error!("Failed to forward strategy event to risk actor: {}", e);
                        } else if event_count % 10 == 0 {
                            debug!("Forwarded {} strategy events to risk actor", event_count);
                        }
                    }
                }
                
                warn!("Strategy → Risk event forwarder terminated after {} events", event_count);
            });
        }
        
        // 3. Risk → Execution (risk assessments flow to execution)
        let (execution_risk_sender, mut execution_risk_receiver) = mpsc::channel::<crate::actors::Event>(100);
        info!("Creating subscription: Risk events → Execution actor");
        self.message_bus.subscribe("risk".to_string(), execution_risk_sender).await?;
        
        // Create a dedicated task for the Execution actor to process risk events
        if let Some(ref execution_ref) = self.execution_actor_ref {
            let execution_ref = execution_ref.clone();
            tokio::spawn(async move {
                info!("🔄 Risk → Execution event forwarder started");
                let mut event_count = 0;
                
                while let Some(event) = execution_risk_receiver.recv().await {
                    // Check for forced shutdown
                    if is_forced_shutdown() {
                        info!("Force shutdown detected, terminating Risk → Execution event forwarder");
                        break;
                    }
                    
                    event_count += 1;
                    if let crate::actors::Event::Risk(risk_event) = event.clone() {
                        // Forward the risk event to the execution actor
                        if let Err(e) = execution_ref.send(crate::actors::Message::Event(event)).await {
                            error!("Failed to forward risk event to execution actor: {}", e);
                        } else if event_count % 10 == 0 {
                            debug!("Forwarded {} risk events to execution actor", event_count);
                        }
                    }
                }
                
                warn!("Risk → Execution event forwarder terminated after {} events", event_count);
            });
        }
        
        // 4. Database logging - DB actor receives all events for record-keeping
        let db_market_sender = mpsc::channel::<crate::actors::Event>(100).0;
        let db_strategy_sender = mpsc::channel::<crate::actors::Event>(100).0;
        let db_risk_sender = mpsc::channel::<crate::actors::Event>(100).0;
        let db_execution_sender = mpsc::channel::<crate::actors::Event>(100).0;
        
        info!("Creating database subscriptions for all event types");
        self.message_bus.subscribe("market".to_string(), db_market_sender).await?;
        self.message_bus.subscribe("strategy".to_string(), db_strategy_sender).await?;
        self.message_bus.subscribe("risk".to_string(), db_risk_sender).await?;
        self.message_bus.subscribe("execution".to_string(), db_execution_sender).await?;
        
        // Very important: Subscribe the database actor directly to execution events
        if let Some(database_ref) = &self.database_actor_ref {
            let database_ref_clone = database_ref.clone();
            info!("🔄 Setting up direct execution event subscription for DatabaseActor");
            
            let (direct_execution_sender, mut direct_execution_receiver) = mpsc::channel::<crate::actors::Event>(100);
            self.message_bus.subscribe("execution".to_string(), direct_execution_sender).await?;
            
            tokio::spawn(async move {
                info!("🔄 Execution → Database direct forwarder started");
                let mut event_count = 0;
                
                while let Some(event) = direct_execution_receiver.recv().await {
                    // Check for forced shutdown
                    if is_forced_shutdown() {
                        info!("Force shutdown detected, terminating Execution → Database event forwarder");
                        break;
                    }
                    
                    event_count += 1;
                    if let crate::actors::Event::Execution(execution_event) = &event {
                        info!("Forwarding execution event directly to database actor: {:?}", execution_event);
                        if let Err(e) = database_ref_clone.send(crate::actors::Message::Event(event.clone())).await {
                            error!("Failed to forward execution event to database actor: {}", e);
                        } else if event_count % 5 == 0 {
                            debug!("Forwarded {} execution events to database actor", event_count);
                        }
                    }
                }
                
                warn!("Execution → Database event forwarder terminated after {} events", event_count);
            });
        }
        
        // Verify all subscriptions after setup
        let market_subs = self.message_bus.debug_subscriber_count("market").await;
        let strategy_subs = self.message_bus.debug_subscriber_count("strategy").await;
        let risk_subs = self.message_bus.debug_subscriber_count("risk").await;
        let execution_subs = self.message_bus.debug_subscriber_count("execution").await;
        let database_subs = self.message_bus.debug_subscriber_count("database").await;
        
        info!("Final MessageBus subscriber counts - market: {}, strategy: {}, risk: {}, execution: {}, database: {}", 
            market_subs, strategy_subs, risk_subs, execution_subs, database_subs);
        
        // Start the actors
        if let Some(ref market_ref) = self.market_actor_ref {
            market_ref.send(crate::actors::Message::Command(crate::actors::Command::Start)).await?;
        }
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            strategy_ref.send(crate::actors::Message::Command(crate::actors::Command::Start)).await?;
        }
        if let Some(ref risk_ref) = self.risk_actor_ref {
            risk_ref.send(crate::actors::Message::Command(crate::actors::Command::Start)).await?;
        }
        if let Some(ref execution_ref) = self.execution_actor_ref {
            execution_ref.send(crate::actors::Message::Command(crate::actors::Command::Start)).await?;
        }
        if let Some(ref database_ref) = self.database_actor_ref {
            database_ref.send(crate::actors::Message::Command(crate::actors::Command::Start)).await?;
        }
        
        self.running = true;
        info!("Actor-based trading bot system started successfully");
        
        Ok(())
    }
    
    /// Stop the trading bot system
    pub async fn stop(&mut self) -> Result<(), Error> {
        if !self.running {
            return Err(Error::InvalidInput("Trading bot is not running".to_string()));
        }
        
        info!("Stopping actor-based trading bot system");
        
        // Stop all actors
        if let Some(ref market_ref) = self.market_actor_ref {
            market_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)).await?;
        }
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            strategy_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)).await?;
        }
        if let Some(ref risk_ref) = self.risk_actor_ref {
            risk_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)).await?;
        }
        if let Some(ref execution_ref) = self.execution_actor_ref {
            execution_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)).await?;
        }
        if let Some(ref database_ref) = self.database_actor_ref {
            database_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)).await?;
        }
        
        self.running = false;
        info!("Actor-based trading bot system stopped successfully");
        
        Ok(())
    }
    
    /// Check if the trading bot is running
    pub fn is_running(&self) -> bool {
        self.running
    }
    
    /// Get the status of the trading bot system
    pub async fn get_status(&self) -> Result<serde_json::Value, Error> {
        let mut status = json!({
            "running": self.running,
            "paper_trading": self.config.trading.paper_trading,
            "actors": {
                "market": false,
                "strategy": false,
                "risk": false,
                "execution": false,
                "database": false
            },
            "strategy_type": "",
            "tokens_tracked": self.get_tokens_to_track(),
            "started_at": "",
        });
        
        // Get status from each actor
        if let Some(ref market_ref) = self.market_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(market_ref).await {
                status["actors"]["market"] = json!(actor_status.contains("running: true"));
            }
        }
        
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(strategy_ref).await {
                status["actors"]["strategy"] = json!(actor_status.contains("running: true"));
                
                // Try to extract strategy type
                if let Some(start_idx) = actor_status.find("strategy: ") {
                    if let Some(end_idx) = actor_status[start_idx..].find(',') {
                        let strategy_type = &actor_status[start_idx + 10..start_idx + end_idx];
                        status["strategy_type"] = json!(strategy_type);
                    }
                }
            }
        }
        
        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(risk_ref).await {
                status["actors"]["risk"] = json!(actor_status.contains("running: true"));
            }
        }
        
        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(execution_ref).await {
                status["actors"]["execution"] = json!(actor_status.contains("running: true"));
            }
        }
        
        if let Some(ref database_ref) = self.database_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(database_ref).await {
                status["actors"]["database"] = json!(actor_status.contains("running: true"));
            }
        }
        
        Ok(status)
    }
    
    /// Get metrics from the trading bot system
    pub async fn get_metrics(&self) -> Result<serde_json::Value, Error> {
        let mut metrics = json!({
            "running": self.running,
            "paper_trading": self.config.trading.paper_trading,
            "market": null,
            "strategy": null,
            "risk": null,
            "execution": null,
            "database": null,
        });
        
        // Get metrics from each actor
        if let Some(ref market_ref) = self.market_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(market_ref).await {
                metrics["market"] = actor_metrics;
            }
        }
        
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(strategy_ref).await {
                metrics["strategy"] = actor_metrics;
            }
        }
        
        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(risk_ref).await {
                metrics["risk"] = actor_metrics;
            }
        }
        
        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(execution_ref).await {
                metrics["execution"] = actor_metrics;
            }
        }
        
        if let Some(ref database_ref) = self.database_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(database_ref).await {
                metrics["database"] = actor_metrics;
            }
        }
        
        Ok(metrics)
    }
    
    /// Get the current positions from the execution actor
    pub async fn get_positions(&self) -> Result<Vec<Position>, Error> {
        if !self.running {
            return Err(Error::InvalidInput("Trading bot is not running".to_string()));
        }
        
        if let Some(ref execution_ref) = self.execution_actor_ref {
            let (tx, rx) = tokio::sync::oneshot::channel();
            execution_ref.send(crate::actors::Message::Query(
                crate::actors::Query::GetMetrics, 
                tx
            )).await?;
            
            match rx.await {
                Ok(Ok(crate::actors::QueryResult::Metrics(metrics))) => {
                    if let Some(positions_json) = metrics.get("positions") {
                        if let Some(positions_array) = positions_json.as_array() {
                            let mut positions = Vec::new();
                            for pos in positions_array {
                                if let (Some(token_id), Some(entry_price), Some(size)) = (
                                    pos.get("token_id").and_then(|v| v.as_str()),
                                    pos.get("entry_price").and_then(|v| v.as_f64()),
                                    pos.get("size").and_then(|v| v.as_f64())
                                ) {
                                    positions.push(Position {
                                        token_id: token_id.to_string(),
                                        coingecko_id: token_id.to_string(), // Use token_id as coingecko_id
                                        entry_price,
                                        current_price: entry_price, // Default to entry price
                                        highest_price: entry_price, // Default to entry price
                                        size,
                                        unrealized_pnl: 0.0, // Default to 0
                                        entry_time: Utc::now(), // Default to now
                                    });
                                }
                            }
                            return Ok(positions);
                        }
                    }
                    Err(Error::Parse("Failed to parse positions from metrics".to_string()))
                },
                Ok(Ok(_)) => Err(Error::Parse("Unexpected query result type".to_string())),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(Error::Task(format!("Failed to receive query response: {}", e))),
            }
        } else {
            Err(Error::InvalidInput("Execution actor not initialized".to_string()))
        }
    }
    
    /// Update configuration for all actors
    pub async fn update_config(&self, config: &serde_json::Value) -> Result<(), Error> {
        if !self.running {
            return Err(Error::InvalidInput("Trading bot is not running".to_string()));
        }
        
        info!("Updating configuration for all actors");
        
        // Update each actor with relevant config
        if let Some(ref market_ref) = self.market_actor_ref {
            if let Some(market_config) = config.get("market") {
                market_ref.send(crate::actors::Message::Command(
                    crate::actors::Command::UpdateConfig(market_config.clone())
                )).await?;
            }
        }
        
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Some(strategy_config) = config.get("strategy") {
                strategy_ref.send(crate::actors::Message::Command(
                    crate::actors::Command::UpdateConfig(strategy_config.clone())
                )).await?;
            }
        }
        
        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Some(risk_config) = config.get("risk") {
                risk_ref.send(crate::actors::Message::Command(
                    crate::actors::Command::UpdateConfig(risk_config.clone())
                )).await?;
            }
        }
        
        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Some(execution_config) = config.get("execution") {
                execution_ref.send(crate::actors::Message::Command(
                    crate::actors::Command::UpdateConfig(execution_config.clone())
                )).await?;
            }
        }
        
        if let Some(ref database_ref) = self.database_actor_ref {
            if let Some(database_config) = config.get("database") {
                database_ref.send(crate::actors::Message::Command(
                    crate::actors::Command::UpdateConfig(database_config.clone())
                )).await?;
            }
        }
        
        Ok(())
    }
    
    /// Run the trading bot in the foreground until stopped
    pub async fn run_foreground(&self, state_file_path: &std::path::Path) -> Result<(), Error> {
        use futures::pin_mut;
        
        info!("Running trading bot in foreground mode");
        info!("Watching state file at: {:?}", state_file_path);
        
        let mut checkpoint_count = 0;
        
        // Create a future that will complete when Ctrl+C is pressed
        let ctrl_c = tokio::signal::ctrl_c();
        pin_mut!(ctrl_c);
        
        // Run until the state file is removed, the bot is stopped, or Ctrl+C is received
        loop {
            // Check if we should continue running
            if !self.running || !state_file_path.exists() {
                info!("State file removed or bot stopped, stopping trading bot");
                break;
            }
            
            // Use tokio::select! to wait for either a timeout or Ctrl+C
            tokio::select! {
                // Sleep for a few seconds before checking again
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    // Checkpoint state
                    if let Err(e) = self.checkpoint_state().await {
                        warn!("Failed to checkpoint trading bot state: {}", e);
                    } else {
                        checkpoint_count += 1;
                        if checkpoint_count % 5 == 0 {
                            debug!("Trading bot still running, checkpoint count: {}", checkpoint_count);
                        }
                    }
                },
                // Handle Ctrl+C signal
                ctrl_c_result = &mut ctrl_c => {
                    match ctrl_c_result {
                        Ok(()) => {
                            info!("Received Ctrl+C signal, gracefully stopping trading bot");
                            
                            // We can't call self.stop() directly because run_foreground takes &self
                            // and stop() requires &mut self, so use stop_all_actors_with_timeout instead
                            info!("Sending stop commands to all actors...");
                            self.stop_all_actors_with_timeout().await;
                            
                            // Clean up the state file
                            if let Err(e) = std::fs::remove_file(state_file_path) {
                                warn!("Failed to remove state file: {}", e);
                            }
                            
                            // Set the global shutdown flag to make sure background tasks exit
                            set_forced_shutdown();
                            
                            info!("Trading bot stopped, exiting now");
                            break;
                        },
                        Err(err) => {
                            error!("Error waiting for Ctrl+C: {}", err);
                        }
                    }
                }
            }
        }
        
        info!("Trading bot foreground process exiting after {} checkpoints", checkpoint_count);
        
        // Set force shutdown one final time to ensure all background tasks exit
        set_forced_shutdown();
        
        // Short sleep to allow final cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        Ok(())
    }
    
    /// Stop all actors with a timeout to prevent hanging
    async fn stop_all_actors_with_timeout(&self) {
        info!("Stopping all actors with timeout protection");
        
        // Create a vector of stop futures
        let mut stop_futures = Vec::new();
        
        // Add stop commands for each actor
        if let Some(ref market_ref) = self.market_actor_ref {
            stop_futures.push(market_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)));
        }
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            stop_futures.push(strategy_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)));
        }
        if let Some(ref risk_ref) = self.risk_actor_ref {
            stop_futures.push(risk_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)));
        }
        if let Some(ref execution_ref) = self.execution_actor_ref {
            stop_futures.push(execution_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)));
        }
        if let Some(ref database_ref) = self.database_actor_ref {
            stop_futures.push(database_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)));
        }
        
        // Wait for all actors to stop or timeout after 2 seconds
        match timeout(std::time::Duration::from_secs(2), futures::future::join_all(stop_futures)).await {
            Ok(results) => {
                let success_count = results.iter().filter(|r| r.is_ok()).count();
                info!("Successfully stopped {}/{} actors", success_count, results.len());
                
                if success_count < results.len() {
                    warn!("Some actors failed to stop gracefully, forcing shutdown");
                }
            },
            Err(_) => {
                warn!("Timeout waiting for actors to stop, forcing shutdown");
            }
        }
        
        // Fix the mutable borrow issue - just log without trying to modify supervisor
        if self.supervisor.is_some() {
            info!("Cleaning up supervisor and message bus");
        }
        
        info!("Actors shutdown process complete");
    }
    
    /// Checkpoint the bot's state to ensure it can recover if needed
    async fn checkpoint_state(&self) -> Result<(), Error> {
        if !self.running {
            return Err(Error::InvalidInput("Trading bot is not running".to_string()));
        }
        
        // Get the current status
        let status = match self.get_status().await {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to get status for checkpoint: {}", e);
                return Err(e);
            }
        };
        
        // Get metrics if possible
        let metrics = self.get_metrics().await.unwrap_or_else(|e| {
            warn!("Failed to get metrics for checkpoint: {}", e);
            json!({
                "error": format!("Failed to get metrics: {}", e)
            })
        });
        
        // Create a combined state object
        let checkpoint = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "status": status,
            "metrics": metrics,
            "config": {
                "paper_trading": self.config.trading.paper_trading,
                "scan_interval": self.config.trading.scan_interval,
                "max_position_size": self.config.trading.max_position_size,
                "risk": {
                    "stop_loss_pct": self.config.trading.risk.stop_loss_pct,
                    "take_profit_pct": self.config.trading.risk.take_profit_pct
                }
            }
        });
        
        // Write the checkpoint to a file - use a temp file first and then rename
        let checkpoint_dir = dirs::config_dir()
            .ok_or_else(|| Error::Config("Could not determine configuration directory".to_string()))?
            .join("honeybadger");
        
        // Create directory if it doesn't exist
        if !checkpoint_dir.exists() {
            info!("Creating checkpoint directory at: {:?}", checkpoint_dir);
            std::fs::create_dir_all(&checkpoint_dir)
                .map_err(|e| Error::Config(format!("Failed to create checkpoint directory: {}", e)))?;
        }
        
        let checkpoint_file = checkpoint_dir.join("trading_checkpoint.json");
        let temp_file = checkpoint_dir.join("trading_checkpoint.tmp.json");
        
        // Write to temp file first
        std::fs::write(&temp_file, serde_json::to_string_pretty(&checkpoint)?)
            .map_err(|e| Error::Io(format!("Failed to write checkpoint file: {}", e)))?;
        
        // Rename temp file to checkpoint file (atomic operation on most filesystems)
        std::fs::rename(&temp_file, &checkpoint_file)
            .map_err(|e| Error::Io(format!("Failed to finalize checkpoint file: {}", e)))?;
        
        Ok(())
    }
    
    // Helper methods
    
    /// Create the market API based on config
    fn create_market_api(&self) -> Result<MarketApi, Error> {
        // Use the new_with_default_provider method which automatically selects the best provider
        // based on available API keys and prioritizes those with WebSocket support
        Ok(MarketApi::new_with_default_provider(&self.config))
    }
    
    /// Create a strategy based on the config
    fn create_strategy(&self, strategy_type: &str, params: &serde_json::Value) -> Result<Strategy, Error> {
        let threshold = params.get("threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(self.config.trading.strategy.threshold);
            
        let min_volume = params.get("min_volume")
            .and_then(|v| v.as_f64())
            .unwrap_or(self.config.trading.strategy.min_volume);
            
        let stop_loss = params.get("stop_loss")
            .and_then(|v| v.as_f64())
            .unwrap_or(self.config.trading.risk.stop_loss_pct);
        
        // Extract min_data_points if provided in params
        let min_data_points = params.get("min_data_points")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        
        // Extract risk_tolerance if provided in params
        let risk_tolerance = params.get("risk_tolerance")
            .and_then(|v| v.as_u64())
            .map(|v| v as u8);
        
        // Use the create_strategy factory function with min_data_points and risk_tolerance
        crate::trading::strategy::create_strategy(
            strategy_type, 
            threshold, 
            min_volume, 
            stop_loss, 
            min_data_points,
            risk_tolerance
        )
    }
    
    /// Get the tokens to track based on config
    fn get_tokens_to_track(&self) -> Vec<String> {
        let default_tokens = vec![
            "bitcoin".to_string(),
            "ethereum".to_string(),
            "binance-coin".to_string(),
            "solana".to_string(),
            "cardano".to_string(),
        ];
        
        // Use tokens from config if available, otherwise use defaults
        match &self.config.trading.tokens_to_track {
            Some(tokens) if !tokens.is_empty() => tokens.clone(),
            _ => default_tokens
        }
    }
    
    /// Get status from an actor
    async fn get_actor_status(&self, actor_ref: &crate::actors::ActorRef) -> Result<String, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        actor_ref.send(crate::actors::Message::Query(
            crate::actors::Query::GetStatus,
            tx
        )).await?;
        
        match rx.await {
            Ok(Ok(crate::actors::QueryResult::Status(status))) => Ok(status),
            Ok(Ok(_)) => Err(Error::Parse("Unexpected query result type".to_string())),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(Error::Task(format!("Failed to receive status response: {}", e))),
        }
    }
    
    /// Get metrics from an actor
    async fn get_actor_metrics(&self, actor_ref: &crate::actors::ActorRef) -> Result<serde_json::Value, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        actor_ref.send(crate::actors::Message::Query(
            crate::actors::Query::GetMetrics,
            tx
        )).await?;
        
        match rx.await {
            Ok(Ok(crate::actors::QueryResult::Metrics(metrics))) => Ok(metrics),
            Ok(Ok(_)) => Err(Error::Parse("Unexpected query result type".to_string())),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(Error::Task(format!("Failed to receive metrics response: {}", e))),
        }
    }
} 