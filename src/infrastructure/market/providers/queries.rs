//! GraphQL queries for Uniswap V3 subgraph

/// Query high-quality pools with quality filters
///
/// Parameters:
/// 1. limit - Maximum number of pools to return
/// 2. min_tvl_usd - Minimum TVL in USD
/// 3. min_volume_usd - IGNORED (volumeUSD is all-time cumulative, not useful for filtering)
/// 4. min_liquidity - Minimum liquidity
/// 5. min_tx_count - Minimum transaction count
/// 6. max_created_timestamp - Maximum creation timestamp (filters out new pools)
///
/// Query pools by quality filters (TVL, liquidity, tx count, pool age).
/// NOTE: Cannot filter by 24h volume here - Pool.volumeUSD is all-time cumulative.
/// 24h volume filtering is done in a separate step via poolDayDatas query.
///
/// WETH pair filtering is done in code AFTER this query, before fetching volume data.
pub fn query_quality_pools(
    limit: usize,
    min_tvl_usd: &str,
    min_liquidity: &str,
    min_tx_count: u32,
    max_created_timestamp: i64,
) -> String {
    // Build the where clause - WETH filtering removed due to GraphQL schema limitations
    // The Uniswap V3 subgraph doesn't support OR conditions mixed with other filters
    // We'll filter WETH pairs in code BEFORE fetching volume data instead
    let where_clause = format!(
        r#"totalValueLockedUSD_gt: "{}",
                liquidity_gt: "{}",
                txCount_gt: "{}",
                createdAtTimestamp_lt: "{}""#,
        min_tvl_usd, min_liquidity, min_tx_count, max_created_timestamp
    );

    format!(
        r#"{{
            pools(
                first: {},
                where: {{
                    {}
                }},
                orderBy: totalValueLockedUSD,
                orderDirection: desc
            ) {{
                id
                token0 {{
                    id
                    symbol
                    name
                    decimals
                }}
                token1 {{
                    id
                    symbol
                    name
                    decimals
                }}
                feeTier
                liquidity
                sqrtPrice
                volumeUSD
                totalValueLockedUSD
                tick
                createdAtTimestamp
            }}
        }}"#,
        limit, where_clause
    )
}

/// Query pools for specific tokens (by address or symbol)
///
/// Parameters:
/// 1. where_conditions - Comma-separated OR conditions for token filtering
pub fn query_pools_by_tokens(where_conditions: &str) -> String {
    format!(
        r#"{{
            pools(
                first: 500,
                where: {{
                    or: [
                        {{ {} }}
                    ]
                }},
                orderBy: totalValueLockedUSD,
                orderDirection: desc
            ) {{
                id
                token0 {{
                    id
                    symbol
                    name
                    decimals
                }}
                token1 {{
                    id
                    symbol
                    name
                    decimals
                }}
                feeTier
                liquidity
                sqrtPrice
                volumeUSD
                totalValueLockedUSD
                tick
            }}
        }}"#,
        where_conditions
    )
}

/// Query poolDayDatas for specific pools to get 24h volume
///
/// Parameters:
/// 1. pool_ids - List of pool addresses to query (e.g., ["0xabc...", "0xdef..."])
/// 2. min_date - Minimum date timestamp (yesterday to get latest 24h data)
pub fn query_pool_day_data_for_pools(pool_ids: &[String], min_date: i64) -> String {
    let pool_ids_str = pool_ids
        .iter()
        .map(|id| format!("\"{}\"", id.to_lowercase()))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"{{
            poolDayDatas(
                first: 1000,
                orderBy: date,
                orderDirection: desc,
                where: {{
                    pool_in: [{}],
                    date_gt: {}
                }}
            ) {{
                pool {{
                    id
                }}
                volumeUSD
                tvlUSD
                date
            }}
        }}"#,
        pool_ids_str, min_date
    )
}

/// Build token filter conditions for pool queries
///
/// Returns (token_addresses_formatted, token_symbols_formatted)
pub fn build_token_filters(tokens_to_track: &[String]) -> (Vec<String>, Vec<String>) {
    let token_addresses: Vec<String> = tokens_to_track
        .iter()
        .filter(|t| t.starts_with("0x") && t.len() == 42)
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();

    let token_symbols: Vec<String> = tokens_to_track
        .iter()
        .filter(|t| !t.starts_with("0x"))
        .map(|t| format!("\"{}\"", t.to_uppercase()))
        .collect();

    (token_addresses, token_symbols)
}

/// Build WHERE conditions for token-based pool queries
pub fn build_token_where_conditions(tokens_to_track: &[String]) -> Option<String> {
    let (token_addresses, token_symbols) = build_token_filters(tokens_to_track);

    if token_addresses.is_empty() && token_symbols.is_empty() {
        return None;
    }

    let mut where_conditions = Vec::new();

    if !token_addresses.is_empty() {
        where_conditions.push(format!(
            "token0_in: [{}], token1_in: [{}]",
            token_addresses.join(", "),
            token_addresses.join(", ")
        ));
    }

    if !token_symbols.is_empty() {
        where_conditions.push(format!(
            "token0_: {{ symbol_in: [{}] }}, token1_: {{ symbol_in: [{}] }}",
            token_symbols.join(", "),
            token_symbols.join(", ")
        ));
    }

    Some(where_conditions.join(", "))
}
