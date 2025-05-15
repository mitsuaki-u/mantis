#[derive(Debug, Clone)]
pub struct DexPair {
    pub token0: Token,
    pub token1: Token,
    pub price: f64,
    pub volume_24h: f64,
    pub liquidity: f64,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub address: String,
    pub symbol: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct DexStats {
    pub volume_24h: f64,
    pub total_liquidity: f64,
    pub pair_count: usize,
} 