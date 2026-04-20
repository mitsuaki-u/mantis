//! Common test utilities and fixtures shared across integration tests.

use mantis::config::{
    ApiConfig, ApiKeys, CacheConfig, Config, DataCollectionConfig, DatabaseConfig, DexConfig,
    LogsConfig, RpcConfig, TradingConfig,
};

/// Creates a test configuration with safe defaults (paper trading, no external deps)
pub fn test_config() -> Config {
    Config {
        api_keys: ApiKeys {
            infura: Some("test_infura_key".to_string()),
            alchemy: Some("test_alchemy_key".to_string()),
        },
        trading: TradingConfig {
            live_trading: false,
            max_position_size: 100.0,
            min_position_size: 10.0,
            max_total_exposure: 1000.0,
            strategy: "momentum".to_string(),
            signal_confidence_threshold: 0.65,
            min_volume: 10000.0,
            min_liquidity: 50000.0,
            min_pool_transaction_count: 100,
            stop_loss: 5.0,
            take_profit: 10.0,
            max_positions: 3,
            max_daily_loss: 10.0,
            max_drawdown: 20.0,
            max_trade_risk_pct: 2.0,
            min_eth_balance: 0.1,
            tokens_to_track: vec![],
            market_data_provider: "alchemy_uniswap_v3".to_string(),
            max_volatility_24h: 30.0,
            rsi_weight: 0.3,
            macd_weight: 0.3,
            bollinger_weight: 0.2,
            volume_weight: 0.2,
            indicator_profile: "day_trading".to_string(),
            max_tokens_to_scan: 50,
            max_gas_cost_usd: 10.0,
            max_gas_cost_percentage: 5.0,
            transaction_priority: mantis::infrastructure::dex::TransactionPriority::Standard,
            max_execution_price_deviation: 0.05,
            min_portfolio_risk_factor: 0.3,
        },
        database: DatabaseConfig {
            host: "localhost".to_string(),
            port: 5432,
            user: "mantis".to_string(),
            password: None,
            dbname: "mantis_test".to_string(),
            pool_max_size: 5,
        },
        api: ApiConfig::default(),
        data_collection: DataCollectionConfig {
            scan_interval_secs: 60,
            history_days: 7,
            auto_start: false,
        },
        logs: LogsConfig::default(),
        rpc: RpcConfig::default(),
        dex: DexConfig::default(),
        cache: CacheConfig::default(),
    }
}

/// Token address fixtures
pub mod fixtures {
    pub const WETH_ADDRESS: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
    pub const USDC_ADDRESS: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    pub const DAI_ADDRESS: &str = "0x6B175474E89094C44Da98b954EedeAC495271d0F";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_is_paper_trading() {
        let config = test_config();
        assert!(!config.trading.live_trading);
        assert_eq!(config.trading.strategy, "momentum");
    }

    #[test]
    fn test_config_has_no_external_cache() {
        let config = test_config();
        assert!(!config.cache.enabled);
        assert!(!config.data_collection.auto_start);
    }
}
