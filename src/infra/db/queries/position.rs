// Queries related to positions

pub const GET_OPEN_POSITIONS: &str = "
SELECT id, token_id, provider_id, entry_price, current_price, highest_price, size, entry_time, updated_at, unrealized_pnl
FROM positions
WHERE is_paper = $1 AND closed = FALSE
ORDER BY entry_time DESC;
";

// Inserts a new position and returns the generated ID
// If an open position with the same token_id and is_paper exists, it updates it.
pub const INSERT_POSITION: &str = "
INSERT INTO positions (token_id, provider_id, entry_price, current_price, highest_price, size, entry_time, is_paper, unrealized_pnl, created_at, updated_at, closed, close_price, profit, exit_time)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW(), FALSE, NULL, 0.0, NULL)
ON CONFLICT (token_id, is_paper) WHERE closed = FALSE
DO UPDATE SET
    entry_price = EXCLUDED.entry_price, -- Allow updating entry on conflict?
    current_price = EXCLUDED.current_price,
    highest_price = EXCLUDED.highest_price,
    size = EXCLUDED.size,
    entry_time = EXCLUDED.entry_time,
    unrealized_pnl = EXCLUDED.unrealized_pnl,
    updated_at = NOW()
RETURNING id;
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

// Updates an existing open position's details and returns its ID.
pub const UPDATE_OPEN_POSITION_DETAILS: &str = "
UPDATE positions SET
    entry_price = $3,
    current_price = $4,
    highest_price = $5,
    size = $6,
    entry_time = $7,
    unrealized_pnl = $9,
    updated_at = NOW()
WHERE token_id = $1 AND is_paper = $8 AND closed = FALSE
RETURNING id;
";

pub const UPDATE_POSITION: &str = "
UPDATE positions
SET current_price = $1, highest_price = GREATEST(highest_price, $1), unrealized_pnl = $2, updated_at = $3
WHERE token_id = LOWER($4) AND is_paper = $5 AND closed = FALSE;
";

pub const CLOSE_POSITION: &str = "
UPDATE positions
SET closed = TRUE, close_price = $1, profit = $2, exit_time = $3, sell_trade_id = $4, updated_at = NOW()
WHERE id = $5 AND is_paper = $6 AND closed = FALSE;
";

pub const DELETE_OPEN_POSITION: &str = "
DELETE FROM positions
WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE;
";

pub const DELETE_OPEN_POSITION_BY_ID: &str = "
DELETE FROM positions
WHERE id = $1 AND is_paper = $2 AND closed = FALSE;
";

pub const POSITION_EXISTS: &str = "
SELECT COUNT(*)
FROM positions
WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE;
";

// Gets the ID as well
pub const GET_POSITION_BY_TOKEN_ID: &str = "
SELECT id, token_id, provider_id, entry_price, current_price, highest_price, size, entry_time, updated_at, unrealized_pnl
FROM positions
WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE;
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
