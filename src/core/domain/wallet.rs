#[derive(Debug, Clone)]
pub struct WalletInfo {
    pub address: String,
    pub balance: f64,
    pub tokens: Vec<TokenHolding>,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone)]
pub struct TokenHolding {
    pub name: String,
    pub symbol: String,
    pub balance: String,
    pub price_usd: Option<f64>,
    pub value_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    pub hash: String,
    pub timestamp: i64,
    pub tx_type: String,
    pub from: String,
    pub to: String,
    pub value: String,
    pub token_transfers: Option<Vec<TokenTransfer>>,
}

#[derive(Debug, Clone)]
pub struct TokenTransfer {
    pub token: String,
    pub symbol: String,
    pub from: String,
    pub to: String,
    pub value: String,
}
