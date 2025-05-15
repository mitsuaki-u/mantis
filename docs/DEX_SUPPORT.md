# HoneyBadger DEX Support

This document describes the decentralized exchange (DEX) integration features of HoneyBadger and how to use them.

## Overview

HoneyBadger provides robust DEX functionality with three main operation modes:

1. **Paper Trading**: Simulate trades without real blockchain transactions
2. **Testnet Trading**: Execute real transactions on Ethereum testnets (Goerli/Mumbai)
3. **Live Trading**: (Coming soon) Execute trades on mainnet

## Supported Networks

Currently, HoneyBadger's DEX functionality supports the following testnets:

- **Goerli** (Ethereum testnet) - Default network
- **Mumbai** (Polygon testnet) - Alternative network

## Configuration

### Wallet Setup

For testnet or live trading, you must configure a wallet:

```bash
# Option 1: Using environment variables
export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_ENV=MY_PRIVATE_KEY_VAR
export MY_PRIVATE_KEY_VAR=0xYourPrivateKeyHere

# Option 2: Using a file
export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_FILE=/path/to/your/key.txt
```

### Network Configuration

```bash
# Enable testnet mode
export HONEYBADGER_DEX_TESTNET=true

# Specify which testnet to use (optional, defaults to Goerli)
export HONEYBADGER_DEX_NETWORK=goerli  # or mumbai
```

### Infura Integration

For better RPC connectivity, add your Infura API key:

```bash
export HONEYBADGER_INFURA_KEY=your_infura_key
```

## DEX Trading Commands

### Paper Trading

```bash
# Simulate trades without blockchain transactions
honeybadger trading start --strategy momentum --paper
```

### Testnet Trading

```bash
# Execute real transactions on Ethereum testnet
honeybadger trading start --strategy momentum --testnet
```

### With Custom Parameters

```bash
honeybadger trading start --strategy momentum --testnet \
  --max-position 0.1 \
  --threshold 3.0 \
  --risk-tolerance 2
```

## Token Support

When executing trades, HoneyBadger supports these pre-configured testnet tokens by symbol:

- `weth`: Wrapped ETH 
- `usdc`: USD Coin
- `dai`: DAI Stablecoin

You can also specify any token by address:

```bash
# Check balance of a specific token
honeybadger wallet balance --testnet --token 0x07865c6e87b9f70255377e024ace6630c1eaa37f

# Execute a trade with a specific token address
honeybadger trading execute --buy --token 0x07865c6e87b9f70255377e024ace6630c1eaa37f --amount 0.1
```

## Under the Hood

HoneyBadger's DEX integration:

1. Uses the ethers.rs library for Ethereum RPC interactions
2. Connects to the Uniswap V2 router on testnets
3. Executes swaps with appropriate slippage settings
4. Handles token approvals automatically for sell operations
5. Provides transaction hash confirmation for all executed trades

## Command Reference

Check token balances:
```bash
honeybadger wallet balance --testnet
```

Execute a manual buy:
```bash
honeybadger trading execute --testnet --buy --token weth --amount 0.1
```

Execute a manual sell:
```bash
honeybadger trading execute --testnet --sell --token 0x07865c6e87b9f70255377e024ace6630c1eaa37f --amount 0.1
```

View transaction history:
```bash
honeybadger trading history --testnet
```

## Example Workflow

1. Set up your environment:
   ```bash
   export HONEYBADGER_DEX_TESTNET=true
   export HONEYBADGER_INFURA_KEY=your_infura_key
   export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_ENV=MY_KEY
   export MY_KEY=0xYourPrivateKeyHere
   ```

2. Start the trading bot:
   ```bash
   honeybadger trading start --strategy momentum --testnet --max-position 0.01
   ```

3. Monitor positions:
   ```bash
   honeybadger trading positions
   ```

4. View execution history:
   ```bash
   honeybadger trading history --limit 10
   ```

## Security Considerations

- Never use production private keys in your testnet configuration
- Use a dedicated testnet wallet with only testnet funds
- Secure your private key storage, even for testnet operations
- Consider using environment variables over files for private keys 