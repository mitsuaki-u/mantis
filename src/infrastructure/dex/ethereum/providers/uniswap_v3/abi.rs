//! Contract ABIs for Uniswap V3 interactions

use crate::infrastructure::errors::{Error, Result};

/// Simple ERC20 ABI for token operations
const ERC20_ABI_JSON: &str = r#"[{"constant":true,"inputs":[],"name":"name","outputs":[{"name":"","type":"string"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"symbol","outputs":[{"name":"","type":"string"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"decimals","outputs":[{"name":"","type":"uint8"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[{"name":"_owner","type":"address"}],"name":"balanceOf","outputs":[{"name":"balance","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"}]"#;

/// Uniswap V3 Quoter ABI for getting swap quotes
const QUOTER_ABI_JSON: &str = r#"[{"inputs":[{"internalType":"address","name":"tokenIn","type":"address"},{"internalType":"address","name":"tokenOut","type":"address"},{"internalType":"uint24","name":"fee","type":"uint24"},{"internalType":"uint256","name":"amountIn","type":"uint256"},{"internalType":"uint160","name":"sqrtPriceLimitX96","type":"uint160"}],"name":"quoteExactInputSingle","outputs":[{"internalType":"uint256","name":"amountOut","type":"uint256"}],"stateMutability":"nonpayable","type":"function"}]"#;

/// Uniswap V3 SwapRouter ABI for executing swaps
const SWAPROUTER_ABI_JSON: &str = r#"[{"inputs":[{"components":[{"internalType":"address","name":"tokenIn","type":"address"},{"internalType":"address","name":"tokenOut","type":"address"},{"internalType":"uint24","name":"fee","type":"uint24"},{"internalType":"address","name":"recipient","type":"address"},{"internalType":"uint256","name":"deadline","type":"uint256"},{"internalType":"uint256","name":"amountIn","type":"uint256"},{"internalType":"uint256","name":"amountOutMinimum","type":"uint256"},{"internalType":"uint160","name":"sqrtPriceLimitX96","type":"uint160"}],"internalType":"struct ISwapRouter.ExactInputSingleParams","name":"params","type":"tuple"}],"name":"exactInputSingle","outputs":[{"internalType":"uint256","name":"amountOut","type":"uint256"}],"stateMutability":"payable","type":"function"}]"#;

/// WETH contract ABI for wrapping/unwrapping ETH
const WETH_ABI_JSON: &str = r#"[{"constant":false,"inputs":[],"name":"deposit","outputs":[],"payable":true,"stateMutability":"payable","type":"function"},{"constant":false,"inputs":[{"internalType":"uint256","name":"wad","type":"uint256"}],"name":"withdraw","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[{"internalType":"address","name":"","type":"address"}],"name":"balanceOf","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"}]"#;

pub(super) fn load_erc20_abi() -> Result<ethers::abi::Abi> {
    serde_json::from_str::<ethers::abi::Abi>(ERC20_ABI_JSON)
        .map_err(|e| Error::Abi(format!("Failed to parse ERC20 ABI: {}", e)))
}

pub(super) fn load_quoter_abi() -> Result<ethers::abi::Abi> {
    serde_json::from_str::<ethers::abi::Abi>(QUOTER_ABI_JSON)
        .map_err(|e| Error::Abi(format!("Failed to parse Quoter ABI: {}", e)))
}

pub(super) fn load_swaprouter_abi() -> Result<ethers::abi::Abi> {
    serde_json::from_str::<ethers::abi::Abi>(SWAPROUTER_ABI_JSON)
        .map_err(|e| Error::Abi(format!("Failed to parse SwapRouter ABI: {}", e)))
}

pub fn load_weth_abi() -> Result<ethers::abi::Abi> {
    serde_json::from_str::<ethers::abi::Abi>(WETH_ABI_JSON)
        .map_err(|e| Error::Abi(format!("Failed to parse WETH ABI: {}", e)))
}
