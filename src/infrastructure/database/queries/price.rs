// Queries related to price history

pub const INSERT_PRICE: &str = "
INSERT INTO price_history (token_id, price, volume, timestamp)
VALUES ($1, $2, $3, $4)
ON CONFLICT (token_id, timestamp) DO NOTHING; -- Or DO UPDATE if needed
";

pub const GET_LATEST_PRICE: &str = "
SELECT price
FROM price_history
WHERE token_id = $1
ORDER BY timestamp DESC
LIMIT 1;
";

pub const GET_PRICE_HISTORY: &str = "
SELECT price, volume, timestamp
FROM price_history
WHERE token_id = $1
ORDER BY timestamp DESC
LIMIT $2;
";

pub const GET_TOKEN_PRICE_STATS: &str = r#"
WITH RelevantHistory AS (
    SELECT
        price,
        timestamp
    FROM
        price_history
    WHERE
        token_id = $1
        AND timestamp >= NOW() - INTERVAL '24 hours'
    ORDER BY
        timestamp DESC
),
LatestPrice AS (
    SELECT price FROM RelevantHistory LIMIT 1
),
Price24hAgo AS (
    SELECT price
    FROM price_history
    WHERE token_id = $1 AND timestamp <= NOW() - INTERVAL '24 hours'
    ORDER BY timestamp DESC
    LIMIT 1
),
Volume24h AS (
    SELECT COALESCE(SUM(volume), 0.0) as total_volume
    FROM price_history
    WHERE token_id = $1 AND timestamp >= NOW() - INTERVAL '24 hours'
)
SELECT
    CASE
        WHEN (SELECT price FROM Price24hAgo) > 0 THEN
            COALESCE(((SELECT price FROM LatestPrice) - (SELECT price FROM Price24hAgo)) / (SELECT price FROM Price24hAgo) * 100.0, 0.0)
        ELSE 0.0 -- Avoid division by zero or handle NULL if price 24h ago was 0 or non-existent
    END AS price_change_24h,
    COALESCE((SELECT total_volume FROM Volume24h), 0.0) AS volume_24h;
"#;

pub const GET_LATEST_MARKET_DATA: &str = r#"
WITH LatestPrices AS (
    SELECT
        token_id,
        price,
        volume,
        timestamp
    FROM (
        SELECT
            token_id,
            price,
            volume,
            timestamp,
            ROW_NUMBER() OVER(PARTITION BY token_id ORDER BY timestamp DESC) as rn
        FROM price_history
    ) ranked
    WHERE rn = 1
), Prices24hAgo AS (
    SELECT
        token_id,
        price AS price_24h_ago
    FROM (
        SELECT
            token_id,
            price,
            timestamp,
            ROW_NUMBER() OVER(PARTITION BY token_id ORDER BY timestamp DESC) as rn
        FROM price_history
        WHERE timestamp <= NOW() - INTERVAL '24 hours'
          AND timestamp >= NOW() - INTERVAL '48 hours' -- Optimization: Limit lookback for 24h price
    ) ranked_ago
    WHERE rn = 1
), Volume24h AS (
    SELECT
        token_id,
        COALESCE(SUM(volume), 0.0) as volume_24h
    FROM price_history
    WHERE timestamp >= NOW() - INTERVAL '24 hours'
    GROUP BY token_id
)
SELECT
    t.id AS token_id,
    t.symbol,
    t.name,
    COALESCE(lp.price, 0.0) AS price_usd,
    CASE
        WHEN COALESCE(p24.price_24h_ago, 0.0) > 0 THEN
            (COALESCE(lp.price, 0.0) - p24.price_24h_ago) / p24.price_24h_ago * 100.0
        ELSE 0.0 -- Return 0% change if price 24h ago was 0 or unavailable
    END AS price_change_24h,
    COALESCE(v24.volume_24h, 0.0) AS volume_24h
FROM tokens t
LEFT JOIN LatestPrices lp ON t.id = lp.token_id
LEFT JOIN Prices24hAgo p24 ON t.id = p24.token_id
LEFT JOIN Volume24h v24 ON t.id = v24.token_id
WHERE t.is_tracked = TRUE AND t.has_price_data = TRUE -- Use correct column names
ORDER BY volume_24h DESC NULLS LAST; -- Order by volume descending, NULLs last
"#;
