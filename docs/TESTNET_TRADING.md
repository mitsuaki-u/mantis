# Testnet Trading Guide

This guide explains how to use the testnet trading feature in Honeybadger to execute real trades on Ethereum testnets (like Goerli or Polygon Mumbai).

## Setup

To use testnet trading, you need:

1. An Ethereum testnet wallet with private key
2. Some testnet ETH (free from faucets)
3. Infura API key (free tier is sufficient)

### 1. Get an Infura API Key

1. Create an account at [Infura](https://infura.io/)
2. Create a new project and get your API key

### 2. Set up a testnet wallet

If you already have a wallet:
- Find your private key (NEVER use your real mainnet wallet for testing)

If you need a new wallet:
- Create one using MetaMask or other Ethereum wallet tools
- Export the private key
- Get testnet ETH from a faucet like [Goerli Faucet](https://goerlifaucet.com/)

### 3. Configure Honeybadger

There are two ways to configure the testnet trading:

#### Option A: Environment Variables

```bash
# Set your Infura API key
export HONEYBADGER_INFURA_KEY="your_infura_key_here"

# Set network (goerli or mumbai)
export HONEYBADGER_DEX_NETWORK="goerli"

# Set your private key via environment variable
export HONEYBADGER_WALLET_PRIVATE_KEY="your_private_key_here"
export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_ENV="HONEYBADGER_WALLET_PRIVATE_KEY"
```

#### Option B: Configuration File

Create or edit `~/.config/honeybadger/config.json`:

```json
{
  "api_keys": {
    "infura": "your_infura_key_here"
  },
  "dex": {
    "name": "uniswap",
    "network": "goerli",
    "testnet": true,
    "wallet": {
      "private_key_file": "/path/to/your/private_key.txt"
    }
  }
}
```

If using a private key file, ensure it contains only the private key string (no leading 0x) and has secure permissions:

```bash
echo "your_private_key_here" > ~/private_key.txt
chmod 600 ~/private_key.txt
```

## Running Testnet Trading

Start the bot with the `--testnet` flag:

```bash
cargo run -- trading start --strategy momentum --min-data-points 1 --risk-tolerance 5 --threshold 1.0 --testnet
```

The bot will:
1. Connect to the configured testnet
2. Use your wallet to execute real trades on testnet
3. Log all transactions with their transaction hashes

## Supported Testnets

Currently supported testnets:
- Goerli (Ethereum)
- Mumbai (Polygon)

## Viewing Transactions

You can view your testnet transactions on blockchain explorers:

- Goerli: https://goerli.etherscan.io/
- Mumbai: https://mumbai.polygonscan.com/

## Testing with Known Tokens

For testing, we've pre-configured addresses for common testnet tokens:
- WETH (Wrapped ETH)
- USDC
- DAI

When executing trades with these tokens, you can use their symbols instead of addresses.

## Troubleshooting

### Insufficient Funds
Make sure your wallet has enough testnet ETH for transactions. Get more from a faucet.

### Transaction Failures
- Check that you have approved token spending for sell orders
- Ensure you're using valid token addresses
- Verify RPC connectivity to the testnet

### Viewing Logs
Run with `--log-level debug` to see more detailed information about transactions:

```bash
cargo run -- trading start --strategy momentum --testnet --log-level debug
``` 