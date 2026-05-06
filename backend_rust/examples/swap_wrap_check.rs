/// Full auto: swap USDC→USDC.e, wrap→pUSD, CLOB balance
use std::str::FromStr as _;
use std::time::Duration;
use alloy::primitives::{address, U256, Uint};
use alloy::providers::ProviderBuilder;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use anyhow::Result;
use polymarket_client_sdk_v2::clob::types::AssetType;
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client,Config};
use polymarket_client_sdk_v2::{POLYGON,PRIVATE_KEY_VAR};

const RPC: &str = "https://polygon-bor-rpc.publicnode.com";
const CLOB_URL: &str = "https://clob.polymarket.com";
const USDCN: alloy::primitives::Address = address!("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359");
const USDCE: alloy::primitives::Address = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");
const SWAP_R: alloy::primitives::Address = address!("0xE592427A0AEce92De3Edee1F18E0157C05861564"); // Uniswap V3 Router (not Router02)
const ONRAMP: alloy::primitives::Address = address!("0x93070a847efEf7F70739046A929D47a521F5B8ee");

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function approve(address spender, uint256 value) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
    }
    #[sol(rpc)]
    interface ICollateralOnramp {
        function wrap(address _asset, address _to, uint256 _amount) external;
    }
    #[sol(rpc)]
    interface IUniRouter {
        function exactInputSingle(
            ExactInputSingleParams params
        ) external returns (uint256 amountOut);

        struct ExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint24 fee;
            address recipient;
            uint256 deadline;
            uint256 amountIn;
            uint256 amountOutMinimum;
            uint160 sqrtPriceLimitX96;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let pk = std::env::var(PRIVATE_KEY_VAR)
        .or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("🔑 EOA: {eoa:#x}");

    let provider = ProviderBuilder::new().wallet(signer.clone()).connect(RPC).await?;

    let usdcn = IERC20::new(USDCN, provider.clone());
    let usdce = IERC20::new(USDCE, provider.clone());

    let bal_in = usdcn.balanceOf(eoa).call().await?;
    println!("USDC nat: {bal_in}");
    if bal_in.is_zero() { anyhow::bail!("no USDC nativo"); }

    // 1. Approve USDC → SwapRouter
    println!("\n📝 Approve USDC nat → Router");
    usdcn.approve(SWAP_R, U256::MAX).send().await?.watch().await?;
    println!("  ✅");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // 2. Swap exactInputSingle
    let min_out = bal_in * U256::from(995u64) / U256::from(1000u64); // 0.5%
    let dl = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 600;
    println!("\n🔄 Swap USDC nat → USDC.e");
    let router = IUniRouter::new(SWAP_R, provider.clone());
    let params = IUniRouter::ExactInputSingleParams {
        tokenIn: USDCN, tokenOut: USDCE, fee: Uint::<24,1>::from(100u32),
        recipient: eoa, deadline: U256::from(dl),
        amountIn: bal_in, amountOutMinimum: min_out,
        sqrtPriceLimitX96: Uint::<160,3>::ZERO,
    };
    router.exactInputSingle(params).send().await
        .map_err(|e| anyhow::anyhow!("swap: {e}"))?
        .watch().await?;
    println!("  ✅");
    tokio::time::sleep(Duration::from_secs(5)).await;

    let bal_ce = usdce.balanceOf(eoa).call().await?;
    println!("USDC.e: {bal_ce}");
    if bal_ce.is_zero() { anyhow::bail!("swap gave 0 USDC.e"); }

    // 3. Approve USDC.e → Onramp + Wrap
    println!("\n📝 Approve USDC.e → Onramp");
    usdce.approve(ONRAMP, bal_ce).send().await?.watch().await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    println!("🔄 Wrap → pUSD");
    let or = ICollateralOnramp::new(ONRAMP, provider.clone());
    or.wrap(USDCE, eoa, bal_ce).send().await?.watch().await?;
    println!("  ✅");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // 4. CLOB
    println!("\n🔗 CLOB EOA");
    let clob = Client::new(CLOB_URL, Config::default())?
        .authentication_builder(&signer).authenticate().await?;
    let req = BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build();
    clob.update_balance_allowance(req.clone()).await?;
    let b = clob.balance_allowance(req).await?;
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("💰 Balance CLOB: ${bal:.2}");
    if bal > 0.0 { println!("🚀 Listo"); }

    Ok(())
}
