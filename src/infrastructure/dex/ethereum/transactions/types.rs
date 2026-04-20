use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Direction of a swap operation for cleaner API design
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapDirection {
    Buy,  // Buy token (swap stablecoin → token)
    Sell, // Sell token (swap token → stablecoin)
}

/// Transaction details returned from DEX operations
#[derive(Debug, Clone, Serialize)]
pub struct TransactionDetails {
    pub transaction_hash: String,
    pub tx_id: String,
    pub block_number: Option<u64>,
    pub gas_used: Option<u64>,
    pub gas_price: Option<String>,
    pub status: TransactionStatus,
    pub timestamp: DateTime<Utc>,
    pub confirmation_time: Option<DateTime<Utc>>,
    pub network_fee_eth: Option<f64>,
    pub network_fee_usd: Option<f64>,
    pub amount_in: f64,
    pub amount_out: f64,
    pub token_in_address: String,
    pub token_out_address: String,
    pub actual_price: f64,
    pub fees_paid: f64,
    pub fee_currency: String,
}

/// Priority level for transaction processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionPriority {
    Low,
    Medium,
    Standard,
    High,
    Urgent,
}

/// Status of a blockchain transaction
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum TransactionStatus {
    Queued {
        tx_id: String,
        submission_time: chrono::DateTime<chrono::Utc>,
        priority: TransactionPriority,
    },
    Pending {
        tx_id: String,
        submission_time: chrono::DateTime<chrono::Utc>,
        last_checked: chrono::DateTime<chrono::Utc>,
        block_height: Option<u64>,
        retry_count: u32,
    },
    Confirmed {
        tx_id: String,
        details: String,
        confirmations: u64,
        required_confirmations: u64,
        finality_probability: f64,
    },
    Success {
        tx_id: String,
        gas_efficiency: f64,
        details: String,
    },
    Failed {
        tx_id: String,
        reason: String,
        error_code: Option<String>,
        gas_used: Option<u64>,
        revert_reason: Option<String>,
        recovery_suggestion: Option<String>,
    },
    Cancelled,
    Dropped {
        tx_id: String,
    },
    Unknown,
}

/// Network fee information for transaction estimation
#[derive(Debug, Clone, Serialize)]
pub struct NetworkFeeInfo {
    pub gas_limit: u64,
    pub gas_price_gwei: Option<f64>,
    pub estimated_fee_eth: f64,
    pub estimated_fee_usd: f64,
    pub priority: TransactionPriority,
    pub fee_currency_symbol: String,
}

impl NetworkFeeInfo {
    pub fn new(
        gas_limit: u64,
        gas_price_gwei: f64,
        estimated_fee_eth: f64,
        estimated_fee_usd: f64,
        priority: TransactionPriority,
        fee_currency_symbol: String,
    ) -> Self {
        Self {
            gas_limit,
            gas_price_gwei: Some(gas_price_gwei),
            estimated_fee_eth,
            estimated_fee_usd,
            priority,
            fee_currency_symbol,
        }
    }
}

impl TransactionDetails {
    pub fn new(
        transaction_hash: String,
        status: TransactionStatus,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            transaction_hash: transaction_hash.clone(),
            tx_id: transaction_hash,
            block_number: None,
            gas_used: None,
            gas_price: None,
            status,
            timestamp,
            confirmation_time: None,
            network_fee_eth: None,
            network_fee_usd: None,
            amount_in: 0.0,
            amount_out: 0.0,
            token_in_address: String::new(),
            token_out_address: String::new(),
            actual_price: 0.0,
            fees_paid: 0.0,
            fee_currency: "ETH".to_string(),
        }
    }

    pub fn with_amount_out(mut self, amount_out: f64) -> Self {
        self.amount_out = amount_out;
        self
    }

    pub fn with_swap_details(
        mut self,
        amount_in: f64,
        amount_out: f64,
        token_in_address: String,
        token_out_address: String,
        actual_price: f64,
    ) -> Self {
        self.amount_in = amount_in;
        self.amount_out = amount_out;
        self.token_in_address = token_in_address;
        self.token_out_address = token_out_address;
        self.actual_price = actual_price;
        self
    }

    pub fn with_block_info(mut self, block_number: u64) -> Self {
        self.block_number = Some(block_number);
        self
    }

    pub fn with_gas_info(mut self, gas_used: u64, gas_price: u64) -> Self {
        self.gas_used = Some(gas_used);
        self.gas_price = Some(gas_price.to_string());
        self
    }

    pub fn with_fees(mut self, network_fee_eth: f64, network_fee_usd: f64) -> Self {
        self.network_fee_eth = Some(network_fee_eth);
        self.network_fee_usd = Some(network_fee_usd);
        self
    }
}
