# Setting Up Testnet Trading in HoneyBadger

This guide explains how to set up and use the testnet DEX client for trading with HoneyBadger.

## Overview

The testnet functionality allows you to test your trading strategies with real transactions but on a test network (e.g., Goerli, Mumbai) rather than mainnet. This helps you verify your trading logic without risking real assets.

## Configuration

### Setting Up Your Wallet

The testnet DEX client requires a wallet private key for executing transactions. You can provide this in two ways:

1. **Using an environment variable**:
   ```bash
   # Set the environment variable containing your private key
   export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_ENV=MY_PRIVATE_KEY_VAR
   
   # Then set the actual private key
   export MY_PRIVATE_KEY_VAR=0xYourPrivateKeyHere
   ```

2. **Using a file**:
   ```bash
   # Set the path to a file containing your private key
   export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_FILE=/path/to/your/key.txt
   ```
   The file should contain only your private key.

### Enabling Testnet Mode

To enable testnet mode:

```bash
# Enable testnet mode
export HONEYBADGER_DEX_TESTNET=true

# Optionally specify a network (defaults to Goerli if not specified)
export HONEYBADGER_DEX_NETWORK=goerli  # or mumbai
```

### Command Line Setup

Alternatively, you can enable testnet mode directly from the command line:

```bash
cargo run -- trading start --testnet
```

## Supported Networks

HoneyBadger testnet mode currently supports:

- **Goerli (Ethereum)**: Default testnet
- **Mumbai (Polygon)**: Alternative testnet for Polygon

## Infura API Key

For testnet operations, an Infura API key is recommended:

```bash
export HONEYBADGER_INFURA_KEY=your_infura_key_here
```

## How It Works

When testnet mode is enabled and a wallet is properly configured:

1. HoneyBadger creates a testnet DEX client
2. The client connects to the specified network (Goerli by default)
3. The wallet is loaded using the provided private key
4. Trade execution happens through the Uniswap V2 router on the testnet
5. Orders are executed as real transactions on the testnet blockchain

## Example Trading Command

```bash
# Start trading with testnet mode enabled
cargo run -- trading start --testnet --strategy momentum --max-position 0.1 --risk-tolerance 2
```

## Troubleshooting

### Common Issues

- **"No wallet configuration found"**: You need to set up either `HONEYBADGER_DEX_WALLET_PRIVATE_KEY_ENV` or `HONEYBADGER_DEX_WALLET_PRIVATE_KEY_FILE`
- **"Cannot load private key"**: The environment variable or file specified doesn't exist or can't be read
- **"Failed to create provider"**: Check your network connection and Infura API key
- **"Invalid private key"**: The private key format is incorrect, it should start with '0x' and be a valid Ethereum private key

### Checking Wallet Connection

To verify your wallet is properly connected, check the logs for:
```
🔑 Successfully connected wallet for testnet trading
```

If you see a warning like:
```
⚠️ No wallet configuration found - testnet trading will not work without a wallet
```
You need to review your wallet configuration.

## Security Notes

- Never store private keys in plaintext in production environments
- Consider using a dedicated testnet wallet with only test funds
- Remember that while testnets use test tokens, your transactions are still public 