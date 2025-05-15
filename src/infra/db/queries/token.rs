// Queries related to tokens

// Upserts a token, updating metadata if it exists, otherwise inserting.
// Assumes 'decimals' is always 0, 'active' and 'tracked' are true for now.
pub const UPSERT_TOKEN: &str = "
INSERT INTO tokens (id, name, symbol, decimals, active, tracked, created_at, updated_at)
VALUES (LOWER($1), $2, $3, 0, $5, $6, NOW(), $4)
ON CONFLICT (id) DO UPDATE SET
    name = EXCLUDED.name,
    symbol = EXCLUDED.symbol,
    decimals = EXCLUDED.decimals, -- Update decimals if needed
    active = EXCLUDED.active,
    tracked = EXCLUDED.tracked,
    updated_at = EXCLUDED.updated_at;
";

// Updates specific metadata fields for an existing token.
pub const UPDATE_TOKEN_METADATA: &str = "
UPDATE tokens
SET symbol = $2, name = $3, updated_at = $4
WHERE id = LOWER($1);
";

// Inserts a basic token record if it doesn't exist, ignoring if it does.
// Used when only the token ID is known initially.
pub const INSERT_TOKEN_SIMPLE: &str = "
INSERT INTO tokens (id, created_at, updated_at)
VALUES (LOWER($1), $2, $2)
ON CONFLICT (id) DO NOTHING;
";

// Retrieves the symbol and name for a given token ID.
pub const GET_TOKEN_INFO: &str = "
SELECT symbol, name
FROM tokens
WHERE id = LOWER($1);
";

// Checks if a token exists in the tokens table. Returns count (0 or 1).
pub const CHECK_TOKEN_EXISTS: &str = "
SELECT COUNT(*)
FROM tokens
WHERE id = LOWER($1);
";
