# HoneyBadger - Crypto Trading & Analysis Tool

HoneyBadger is a powerful command-line tool for cryptocurrency market analysis, DEX interaction, wallet tracking, and automated trading.

## Installation

```bash
cargo install honeybadger # Or build from source: cargo build --release
# Ensure the executable is in your PATH
```

## Overview

HoneyBadger provides several main command groups:

- `trading`: Manage the automated trading bot (start, stop, status, history, positions, open, close).
- `dex`: Interact with Decentralized Exchanges (e.g., query pair info, stats).
- `wallet`: Analyze on-chain wallets.
- `config`: Manage HoneyBadger's configuration.
- `db`: Perform database maintenance tasks.

Many commands accept **Global CLI Options** that can override settings from your configuration file for a single run.

## Global CLI Options

These options can be used with most `honeybadger` commands, especially with `trading start`.

- `--paper`: Enable paper trading mode (simulates trades, no real funds).
- `--scan-interval <SECONDS>`: Override market data scan interval.
- `--max-position <USD_AMOUNT>`: Override maximum size for a single position.
- `--max-exposure <USD_AMOUNT>`: Override maximum total exposure across all positions.
- `--strategy <STRATEGY_NAME>`: Override the default trading strategy.
- `--confidence-threshold <VALUE>`: Override strategy signal confidence threshold (e.g., 0.0-1.0, depends on strategy).
- `--min-volume <USD_AMOUNT>`: Override minimum 24h trading volume for a token to be considered.
- `--stop-loss <PERCENTAGE>`: Override stop-loss percentage (e.g., `5.0` for 5%).
- `--take-profit <PERCENTAGE>`: Override take-profit percentage (e.g., `10.0` for 10%).
- `--max-positions <NUMBER>`: Override maximum number of concurrent open positions.
- `--risk-tolerance <LEVEL>`: Override risk tolerance level (0-5, higher is more aggressive).
- `--coingecko-key <API_KEY>`: Provide CoinGecko API key directly.
- `--enable-cache`: Force enable Redis cache.
- `--redis-url <URL>`: Override Redis URL.
- `--cache-flush-interval <SECONDS>`: Override cache flush interval.

### Logging Configuration

HoneyBadger provides flexible logging options:

- `--debug`: Enable debug level logging (shortcut for `--log-level debug`).
- `--log-level <LEVEL>`: Set verbosity (error, warn, info, debug, trace, trade).
  ```bash
  honeybadger trading start --log-level trace
  ```
- `--log-file <FILENAME>`: Save logs to a specified file. Relative paths are inside the configured logs directory; absolute paths are used as is.
  ```bash
  honeybadger trading start --log-file trading_session.log
  ```
- `--log-modules <MODULES>`: Filter logs for specific modules (comma-separated).
  ```bash
  honeybadger trading start --log-modules "honeybadger::trading::strategy,honeybadger::infra::actors::execution" --log-level debug
  ```
> When you use `--log-level` or `--debug` without `--log-file`, HoneyBadger automatically creates a timestamped log file in your configured logs directory.

Default log file locations:
- Linux: `~/.local/share/honeybadger/logs/`
- macOS: `~/Library/Application Support/honeybadger/logs/`
- Windows: `C:\Users\[username]\AppData\Roaming\honeybadger\logs\`

Customize logs directory: `honeybadger config set-logs /path/to/custom/logs`

## Configuration (`config`) Commands

Manage HoneyBadger's settings.

- **Show current configuration**:
  ```bash
  honeybadger config show
  honeybadger config show --show-secrets # To reveal sensitive values
  honeybadger config show --json         # For JSON output
  ```
- **Get specific setting**:
  ```bash
  honeybadger config get trading.paper_trading
  honeybadger config get api_keys.coingecko
  ```
- **Set specific setting**:
  ```bash
  honeybadger config set trading.paper_trading true
  honeybadger config set trading.strategy.threshold 0.65
  ```
- **Set API keys**: (Example for CoinGecko)
  ```bash
  honeybadger config set-key coingecko YOUR_COINGECKO_API_KEY
  # Other keys: cryptocompare, etherscan etc.
  ```
- **Bulk set trading parameters**:
  ```bash
  honeybadger config set-trading --paper-trading true --max-position 200 --max-exposure 1000 --stop-loss 3.5
  ```
- **Bulk set strategy parameters**:
  ```bash
  honeybadger config set-strategy --strategy-type momentum --threshold 0.7 --min-volume 50000
  ```
- **Bulk set risk parameters**:
  ```bash
  honeybadger config set-risk --stop-loss 2.5 --take-profit 7.5 --max-positions 3
  ```
- **Configure tokens to track** (for non-wide-scan mode):
  ```bash
  honeybadger config set trading.tokens_to_track '["bitcoin", "ethereum", "solana"]'
  # (Ensure to use proper shell escaping for JSON array)
  ```
- **Show config file path**:
  ```bash
  honeybadger config path
  ```
- **Reset configuration to defaults**:
  ```bash
  honeybadger config reset
  honeybadger config reset --force # Skip confirmation
  ```
- **Set default logs directory**:
  ```bash
  honeybadger config set-logs /var/log/honeybadger
  ```
- **Set default DEX**:
  ```bash
  honeybadger config set-dex --name uniswap --version v3 --network ethereum
  ```

For a complete configuration guide, see `CONFIG.md` (if available, or explore with `honeybadger config show`).

## Trading (`trading`) Commands

Interact with the automated trading bot.

### `trading start`
Start the trading bot. Most global CLI options apply here to override configurations for the session.

- **Basic start (using settings from config file)**:
  ```bash
  honeybadger trading start
  ```
- **Start in paper trading mode**:
  ```bash
  honeybadger trading start --paper
  ```
- **Start in testnet mode (real trades on a test network)**:
  ```bash
  honeybadger trading start --network goerli
  ```
  _Note: `--paper` and `--network` can be used together. Paper trading simulates trades without blockchain transactions, while `--network` specifies which network to use for real trades._

- **Specify trading strategy**:
  ```bash
  honeybadger trading start --strategy momentum
  # Other strategies might be available.
  ```
- **Set position limits**:
  ```bash
  honeybadger trading start --max-position 150 --max-exposure 1000
  # Max single position size: $150, Max total value of all positions: $1000
  ```
- **Set strategy parameters**:
  ```bash
  honeybadger trading start --confidence-threshold 0.6 --min-volume 50000 --stop-loss 3.0
  # Confidence: 0.6 (0.0-1.0 range), Min 24h Vol: $50k, Stop Loss: 3%
  ```
- **Adjust market scan interval**:
  ```bash
  honeybadger trading start --interval 30
  # Scan market data every 30 seconds (default is 60s).
  ```
- **Run in background (daemon mode)**:
  ```bash
  honeybadger trading start --background
  ```
- **Enable wide scan mode (process all available tokens, not just tracked ones)**:
  ```bash
  honeybadger trading start --wide-scan
  ```
- **Set advanced strategy parameters**:
  ```bash
  honeybadger trading start --min-data-points 10 --risk-tolerance 2
  # Min data points for analysis: 10, Risk tolerance: 2 (Moderate)
  ```
- **Set indicator weights (if supported by strategy, e.g., momentum)**:
  ```bash
  honeybadger trading start --strategy momentum --rsi-weight 0.4 --macd-weight 0.25 --bollinger-weight 0.15 --volume-weight 0.2
  ```
- **Use a specific testing mode for signal generation**:
  ```bash
  honeybadger trading start --testing-mode fast
  # Options: production, fast, ultra, mock
  ```
- **Combined example**:
  ```bash
  honeybadger trading start --paper --strategy momentum --max-position 250 --max-exposure 1500 --stop-loss 2.5 --interval 45 --log-level debug --log-file paper_session.log
  ```

### `trading status`
Show the current status of the trading bot, including active actors and summary.

- **Get status**:
  ```bash
  honeybadger trading status
  ```

### `trading health`
Get a health report from the trading bot supervisor, often including actor states.

- **Get health report**:
  ```bash
  honeybadger trading health
  ```

### `trading restart`
Restart a specific actor within the trading system.

- **Restart the strategy actor**:
  ```bash
  honeybadger trading restart --actor-id strategy
  ```
- **Restart the market data actor**:
  ```bash
  honeybadger trading restart --actor-id market
  ```
  _Common actor IDs: market, strategy, risk, execution, database._

### `trading stop`
Stop the trading bot gracefully.

- **Stop the bot**:
  ```bash
  honeybadger trading stop
  ```

### `trading history`
View trading history and performance.

- **View last 10 trades (default)**:
  ```bash
  honeybadger trading history
  ```
- **View last 50 trades**:
  ```bash
  honeybadger trading history --limit 50
  ```
- **View paper trading history**:
  ```bash
  honeybadger trading history --paper
  ```
- **View live trading history**:
  ```bash
  honeybadger trading history --live
  ```
  _Note: If neither `--paper` nor `--live` is specified, it may default to live or configured mode. `--live` implies not paper._

- **View last 20 paper trades**:
  ```bash
  honeybadger trading history --paper --limit 20
  ```

### `trading positions`
View current open positions.

- **View all open positions (defaulting to configured mode - live/paper)**:
  ```bash
  honeybadger trading positions
  ```
- **View open paper trading positions**:
  ```bash
  honeybadger trading positions --paper
  ```
- **View open live trading positions**:
  ```bash
  honeybadger trading positions --live
  ```

### `trading open`
Manually open a new position by executing a buy order and recording it in the database.

**⚠️ Important**: This command executes actual trades (buy orders) on the configured exchange/DEX. For paper trading, it simulates the trade. For live/testnet trading, it executes real transactions.

- **Open a paper position for a specific token with USD amount**:
```bash
  honeybadger trading open --token "SOL-USDC" --amount 100.0 --paper
  # Spend $100 to buy SOL-USDC tokens
```
- **Open a live position with a price limit**:
```bash
  honeybadger trading open --token "ETH-USDT" --amount 250.0 --price 3000.0 --live
  # Spend $250 to buy ETH-USDT, but only if price is $3000 or below
```
- **Open a position at market price**:
```bash
  honeybadger trading open --token "BTC-USDT" --amount 500.0 --live
  # Spend $500 to buy BTC-USDT at current market price
```

**What happens when you run this command**:
1. **Validates the token and amount** and checks for existing positions
2. **Checks current market price** and validates against price limit (if provided)
3. **Executes a buy order** on the configured DEX/exchange to purchase tokens
4. **Records the new position** in the database with actual execution details
5. **Shows detailed results** including transaction ID, tokens received, and effective price

**Error handling**: If the buy order fails, no position is created. If the buy order succeeds but database recording fails, you'll get a warning with the transaction ID for manual reconciliation.

### `trading close`
Manually close an open position by executing a sell order and updating the database.

**⚠️ Important**: This command executes actual trades (sell orders) on the configured exchange/DEX. For paper trading, it simulates the trade. For live/testnet trading, it executes real transactions.

- **Close a paper position for a specific token at a given price**:
```bash
  honeybadger trading close --token "SOL-USDC" --price 145.50 --paper
  # Replace "SOL-USDC" with the actual token ID or pair symbol used by the system.
```
- **Close a live position**:
```bash
  honeybadger trading close --token "ETH-USDT" --price 3050.75 --live
  ```
- **Close a position at market price** (price will be fetched automatically):
```bash
  honeybadger trading close --token "BTC-USDT" --live
  ```

**What happens when you run this command**:
1. **Finds the open position** in the database for the specified token
2. **Executes a sell order** on the configured DEX/exchange to sell all tokens in the position
3. **Updates the database** with the actual execution details (price, quantity, transaction ID)
4. **Shows detailed results** including P&L, transaction ID, and amounts

**Error handling**: If the sell order fails, the database is not updated and your position remains open. If the sell order succeeds but database update fails, you'll get a warning with the transaction ID for manual reconciliation.

_Note: The `--token` argument should be the identifier the system uses for the position (e.g., canonical token ID or pair symbol)._

## DEX Testnet Trading Setup

HoneyBadger can execute real trades on supported test networks, allowing you to test your strategies and configurations without risking real funds. Here's how to set it up:

**1. Configure your Wallet Private Key:**

You need to provide a private key for the wallet HoneyBadger will use for testnet transactions. **NEVER use a private key associated with real funds for testnet trading.** Always use a dedicated testnet wallet.

There are two ways to configure this:

*   **Via an Environment Variable (Recommended for security):**
    1.  Set the actual private key in an environment variable:
        ```bash
        export MY_TESTNET_PRIVATE_KEY="0xYourTestnetPrivateKeyHere"
        ```
    2.  Tell HoneyBadger to use this environment variable by setting it in your `config.json` or using the `config set` command:
        ```bash
        honeybadger config set dex.wallet.private_key_env MY_TESTNET_PRIVATE_KEY
        ```
        This updates your `config.json` like so:
        ```json
        {
          // ... other configs
          "dex": {
            // ... other dex configs
            "wallet": {
              "private_key_env": "MY_TESTNET_PRIVATE_KEY",
              "private_key_file": null
            }
          }
        }
        ```

*   **Via a File Path:**
    1.  Store your private key in a secure text file (e.g., `/path/to/secure/testnet_key.txt`). Ensure this file is appropriately permissioned.
    2.  Tell HoneyBadger the path to this file:
        ```bash
        honeybadger config set dex.wallet.private_key_file /path/to/secure/testnet_key.txt
        ```
        This updates your `config.json`:
        ```json
        {
          // ... other configs
          "dex": {
            // ... other dex configs
            "wallet": {
              "private_key_env": null,
              "private_key_file": "/path/to/secure/testnet_key.txt"
            }
          }
        }
        ```

**2. Configure Network and RPC Provider:**

*   **Select Testnet Network:**
    Specify which testnet you want to use. Supported networks include "goerli" (default), "sepolia", "mumbai".
    ```bash
    honeybadger config set dex.network sepolia
    ```
    This can also be overridden at runtime if the CLI supports it, but usually set in config.

*   **RPC Provider (Infura Recommended):**
    For reliable connection to the blockchain, an RPC provider like Infura is recommended.
    1.  Get an Infura API key from [infura.io](https://infura.io).
    2.  Set your Infura API key in HoneyBadger's configuration:
        ```bash
        honeybadger config set api_keys.infura YOUR_INFURA_API_KEY
        ```
    If you have a custom RPC URL for your chosen testnet, you can set it via:
    ```bash
    honeybadger config set dex.custom_rpc_url YOUR_CUSTOM_RPC_URL
    ```
    If `custom_rpc_url` is set, it will be used instead of the default Infura URL for the selected network.

**3. Get Testnet Funds:**

You'll need testnet currency (e.g., Goerli ETH, Sepolia ETH, Mumbai MATIC) and testnet versions of any tokens you wish to trade. Search online for faucets for your chosen testnet, for example:
    *   Goerli ETH Faucet (e.g., [goerlifaucet.com](https://goerlifaucet.com/), [alchemy.com/gofaucet](https://www.alchemy.com/gofaucet))
    *   Sepolia ETH Faucet (e.g., [sepoliafaucet.com](https://sepoliafaucet.com/), [infura.io/faucet/sepolia](https://www.infura.io/faucet/sepolia))
    *   Mumbai MATIC Faucet (e.g., [faucet.polygon.technology](https://faucet.polygon.technology/))
    Some DEXs on testnets also provide faucets for test tokens.

**4. Start Trading in Testnet Mode:**

Once configured, use the `--network` flag when starting the trading bot:

```bash
honeybadger trading start --network goerli
# Or with additional parameters
honeybadger trading start --network goerli --strategy momentum --max-position 0.1 --log-level info
```
The bot will now attempt to execute real transactions on the configured testnet using your provided wallet and RPC settings.

**Important Security Notes for Testnet Trading:**
*   **NEVER use a mainnet private key for testnet activities.** Create a new, separate wallet exclusively for testnet usage.
*   Testnet funds have no real-world value.
*   Ensure any file containing a private key is stored securely and has restricted permissions. Using environment variables for the key itself is generally safer.

## DEX (`dex`) Commands
Interact with Decentralized Exchanges. (Examples based on common DEX functionalities; actual commands/flags might vary.)

- **View token/pair information on a DEX**:
  ```bash
  honeybadger dex pair 0x6b175474e89094c44da98b954eedeac495271d0f ethereum
  # (Token address and network)
  ```
- **Get DEX statistics**:
  ```bash
  honeybadger dex stats raydium solana
  # (DEX name and network)
  ```
Use `honeybadger dex --help` for detailed subcommands and options.

## Wallet (`wallet`) Commands
Analyze on-chain wallet information.

- **Get wallet information (defaulting to Ethereum)**:
  ```bash
  honeybadger wallet info 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
  ```
- **Specify chain**:
  ```bash
  honeybadger wallet info 0xSomeWalletAddressOnPolygon --chain polygon
  ```
Use `honeybadger wallet --help` for detailed subcommands and options.


## Database (`db`) Commands
Manage the application's database.

- **Reset the database** (deletes and recreates schema; **Warning: Deletes all data**):
  ```bash
  honeybadger db reset
  # You will be asked for confirmation.
  ```
- **Display database schema**:
  ```bash
  honeybadger db schema
  ```
- **Write schema to a file**:
  ```bash
  honeybadger db schema --output db_schema.txt
  ```
  _Note: Schema export functionality for PostgreSQL might be under development._

- **Run database maintenance tasks** (e.g., VACUUM, ANALYZE):
  ```bash
  honeybadger db maintenance
  ```
- **Start periodic database maintenance scheduler**:
  ```bash
  honeybadger db period-start
  ```
  _Note: Periodic maintenance scheduler for PostgreSQL might be under development._

## Understanding Token Identifiers

Internally, HoneyBadger may distinguish between:
- **Canonical Token ID**: A normalized, system-wide identifier for a token (e.g., "bitcoin", "ethereum_solana_usdc_pool").
- **Provider Token ID**: The specific ID or address used by a data provider or DEX (e.g., a contract address).

Most user-facing commands will expect a common symbol or canonical ID. The system handles the mapping to provider-specific IDs internally.

## Development

(Retain or update this section as per project status)

### Setup
```bash
git clone <repository-url>
cd honeybadger
cargo build
```

### Running Tests
```bash
cargo test
```

### Contributing
Contributions are welcome! Please follow the existing code style and submit pull requests.

## License
(Specify License - e.g., MIT, Apache 2.0)