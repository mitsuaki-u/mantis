// Queries related to trades

pub const INSERT_TRADE_WITH_POSITION_ID: &str = "
INSERT INTO trades (token_id, provider_id, price, size, timestamp, is_buy, is_paper, position_id)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
RETURNING id;
";

// Used for both standalone buys/sells and position closes
pub const INSERT_TRADE: &str = "
INSERT INTO trades (token_id, provider_id, price, size, is_buy, timestamp, is_paper)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING id;
";

// Specifically for recording a SELL trade (used in record_position_close_with_trade)
// It doesn't link a position_id itself, as the position is being closed.
pub const INSERT_TRADE_SELL: &str = "
INSERT INTO trades (token_id, provider_id, price, size, timestamp, is_buy, is_paper, position_id)
VALUES ($1, $2, $3, $4, $5, FALSE, $6, $7)
RETURNING id;
";

pub const GET_TRADES_BY_TOKEN: &str = "
SELECT id, token_id, price, size, is_buy, timestamp, is_paper, position_id
FROM trades
WHERE token_id = $1
ORDER BY timestamp DESC
LIMIT $2;
";

pub const GET_COMPLETED_POSITIONS: &str = "
SELECT
    p.id,              -- 0: Position ID
    p.token_id,        -- 1: Token ID
    p.entry_price,     -- 2: Entry Price
    p.close_price,     -- 3: Exit Price (from position close)
    p.size,            -- 4: Size
    p.entry_time,      -- 5: Entry Time
    p.exit_time,       -- 6: Exit Time (from position close)
    p.profit           -- 7: Profit (already calculated potentially)
FROM
    positions p
WHERE
    p.closed = TRUE
    AND p.is_paper = $1 -- Filter by paper/live
ORDER BY
    p.exit_time DESC
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
