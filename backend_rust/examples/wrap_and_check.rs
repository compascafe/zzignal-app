/// One-shot: wrap USDC→pUSD via CollateralOnramp + verify CLOB balance
use std::str::FromStr as _;
use alloy::primitives::{address, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use anyhow::{Context, Result};
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use std::time::Duration;

const RPC_URL: &str = "https://polygon-bor-rpc.publicnode.com";
const CLOB_URL: &str = "https://clob.polymarket.com";
const ONRAMP: alloy::primitives::Address = address!("0x93070a847efEf7F70739046A929D47a521F5B8ee");
const USDCE: alloy::primitives::Address = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");
const NATIVE_USDC: alloy::primitives::Address = address!("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359");

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
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let pk = std::env::var(PRIVATE_KEY_VAR)
        .or_else(|_| std::env::var("PRIVATE_KEY"))
        .context("Missing POLYMARKET_PRIVATE_KEY")?;

    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("🔑 EOA: {eoa:#x}");

    let provider = ProviderBuilder::new().wallet(signer.clone()).connect(RPC_URL).await?;

    // Check both USDC.e and native USDC balances
    let usdce = IERC20::new(USDCE, provider.clone());
    let native = IERC20::new(NATIVE_USDC, provider.clone());

    let bal_usdce = usdce.balanceOf(eoa).call().await?;
    let bal_native = native.balanceOf(eoa).call().await?;
    println!("USDC.e: {bal_usdce}");
    println!("USDC nativo: {bal_native}");

    // Pick the token with balance
    let (token_label, token_addr, balance) = if bal_native > U256::ZERO {
        ("USDC nativo", NATIVE_USDC, bal_native)
    } else if bal_usdce > U256::ZERO {
        ("USDC.e", USDCE, bal_usdce)
    } else {
        println!("No USDC found");
        return Ok(());
    };

    println!("\n💵 Wrapping {balance} {token_label}...");

    // Approve Onramp
    let token = IERC20::new(token_addr, provider.clone());
    let tx = token.approve(ONRAMP, balance).send().await
        .map_err(|e| anyhow::anyhow!("approve failed: {e}"))?
        .watch().await
        .context("approve not confirmed")?;
    println!("  approve: {tx}");

    tokio::time::sleep(Duration::from_secs(3)).await;

    // Wrap
    let onramp = ICollateralOnramp::new(ONRAMP, provider.clone());
    let tx = onramp.wrap(token_addr, eoa, balance).send().await
        .map_err(|e| anyhow::anyhow!("wrap failed: {e}"))?
        .watch().await
        .context("wrap not confirmed")?;
    println!("  wrap: {tx}");

    // Check CLOB balance with EOA signature type
    println!("\n🔗 Checking CLOB balance (SignatureType::Eoa)...");
    let clob = Client::new(CLOB_URL, Config::default())?
        .authentication_builder(&signer)
        .authenticate().await?;

    let req = BalanceAllowanceRequest::builder()
        .asset_type(AssetType::Collateral)
        .build();
    clob.update_balance_allowance(req.clone()).await?;
    let b = clob.balance_allowance(req).await?;
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("💰 Balance CLOB: raw={raw} → ${bal:.2}");

    if bal > 0.0 {
        println!("🚀 Listo para operar");
    }

    Ok(())
}
