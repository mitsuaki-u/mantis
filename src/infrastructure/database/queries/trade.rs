// Queries related to trades

pub const INSERT_TRADE_WITH_POSITION_ID: &str = "
INSERT INTO trades (token_id, provider_id, price, size, timestamp, is_buy, is_paper, position_id)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (token_id, price, size, timestamp, is_buy, is_paper) DO NOTHING
RETURNING id;
";

// Used for both standalone buys/sells and position closes
pub const INSERT_TRADE: &str = "
INSERT INTO trades (token_id, provider_id, price, size, is_buy, timestamp, is_paper)
VALUES ($1, $2, $3, $4, $5, $6, $7)
ON CONFLICT (token_id, price, size, timestamp, is_buy, is_paper) DO NOTHING
RETURNING id;
";

pub const GET_TRADES_BY_TOKEN: &str = "
SELECT id, token_id, price, size, is_buy, timestamp, is_paper, position_id
FROM trades
WHERE token_id = $1
ORDER BY timestamp DESC
LIMIT $2;
";

// Fetches individual trades, ordered by time
pub const GET_ALL_TRADES_HISTORY: &str = "
SELECT id, token_id, price, size, is_buy, timestamp, is_paper, position_id
FROM trades
WHERE is_paper = $1
ORDER BY timestamp DESC
LIMIT $2;
";

// Specifically for recording a SELL trade (used in record_position_close_with_trade)
// Uses ON CONFLICT DO NOTHING for idempotent operations (duplicate protection via unique index)
pub const INSERT_TRADE_SELL: &str = "
INSERT INTO trades (token_id, provider_id, price, size, timestamp, is_buy, is_paper, position_id)
VALUES ($1, $2, $3, $4, $5, FALSE, $6, $7)
ON CONFLICT (token_id, price, size, timestamp, is_buy, is_paper) DO NOTHING
RETURNING id;
";

pub const GET_EXISTING_SELL_TRADE_ID: &str = "
SELECT id
FROM trades
WHERE token_id = $1 AND price = $2 AND size = $3 AND timestamp = $4 AND is_buy = FALSE AND is_paper = $5;
";
