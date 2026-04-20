// Queries related to positions

pub const GET_OPEN_POSITIONS: &str = "
SELECT id, token_id, provider_id, entry_price, current_price, highest_price, size, entry_time, updated_at, unrealized_pnl
FROM positions
WHERE is_paper = $1 AND closed = FALSE
ORDER BY entry_time DESC;
";

// Attempts to insert a new position. If an open position conflicts, does nothing.
// Returns ID only if a new row was inserted.
pub const INSERT_POSITION_ON_CONFLICT_DO_NOTHING: &str = "
INSERT INTO positions (token_id, provider_id, entry_price, current_price, highest_price, size, entry_time, is_paper, unrealized_pnl, created_at, updated_at, closed, close_price, profit, exit_time)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW(), FALSE, NULL, 0.0, NULL)
ON CONFLICT (token_id, is_paper) WHERE closed = FALSE
DO NOTHING
RETURNING id;
";

pub const UPDATE_POSITION: &str = "
UPDATE positions
SET current_price = $1, highest_price = GREATEST(highest_price, $1), unrealized_pnl = $2, updated_at = $3
WHERE token_id = LOWER($4) AND is_paper = $5 AND closed = FALSE;
";

pub const CLOSE_POSITION: &str = "
UPDATE positions
SET closed = TRUE, close_price = $1, profit = $2, fees_paid = $3, exit_time = $4, sell_trade_id = $5, updated_at = NOW()
WHERE id = $6 AND is_paper = $7 AND closed = FALSE;
";

// NOTE: Use CLOSE_POSITION query with record_position_close_with_trade() method to properly close positions.

pub const POSITION_EXISTS: &str = "
SELECT COUNT(*)
FROM positions
WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE;
";

// Gets the ID as well (LIMIT 1 as defensive safeguard against duplicates)
pub const GET_POSITION_BY_TOKEN_ID: &str = "
SELECT id, token_id, provider_id, entry_price, current_price, highest_price, size, entry_time, updated_at, unrealized_pnl
FROM positions
WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE
LIMIT 1;
";

pub const GET_TOTAL_PNL: &str = "
SELECT COALESCE(SUM(profit), 0.0)
FROM positions
WHERE is_paper = $1 AND closed = TRUE;
";

pub const COUNT_OPEN_POSITIONS: &str = "
SELECT COUNT(*)
FROM positions
WHERE is_paper = $1 AND closed = FALSE;
";

pub const GET_PROVIDER_ID_FOR_TOKEN: &str = "
SELECT provider_id
FROM positions
WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE;
";

pub const CHECK_POSITION_STATUS: &str = "
SELECT closed, close_price, profit, exit_time, provider_id
FROM positions
WHERE id = $1 AND is_paper = $2;
";

pub const GET_POSITION_FOR_PNL_CALC: &str = "
SELECT size, entry_price
FROM positions
WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE;
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
    p.profit,          -- 7: Profit (gross profit)
    p.fees_paid        -- 8: Actual fees paid
FROM
    positions p
WHERE
    p.closed = TRUE
    AND p.is_paper = $1 -- Filter by paper/live
ORDER BY
    p.exit_time DESC
LIMIT $2;
";

pub const HEALTH_CHECK: &str = "SELECT 1";
