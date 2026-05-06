/// Full setup: swap USDC nat→USDC.e, wrap→pUSD, verify CLOB balance
use std::str::FromStr as _;
use std::time::Duration;
use alloy::primitives::{address, U256, Bytes};
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
const USDCNAT: alloy::primitives::Address = address!("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359");
const USDCE: alloy::primitives::Address = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");
const UNI_ROUTER: alloy::primitives::Address = address!("0xE592427A0AEce92De3Edee1F18E0157C05861564");
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
}

use alloy::rpc::types::TransactionRequest;
use alloy::providers::Provider;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("🔑 EOA: {eoa:#x}");

    let provider = ProviderBuilder::new().wallet(signer.clone()).connect(RPC).await?;

    let usdcnat = IERC20::new(USDCNAT, provider.clone());
    let usdce   = IERC20::new(USDCE, provider.clone());
    let bal_nat = usdcnat.balanceOf(eoa).call().await?;
    println!("USDC nat: {bal_nat}");
    if bal_nat.is_zero() { anyhow::bail!("No USDC nativo"); }

    // 1. Approve USDC nat → Uniswap
    println!("\n📝 Approve USDC nat → Router");
    usdcnat.approve(UNI_ROUTER, bal_nat).send().await?.watch().await?;
    println!("  ✅");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // 2. Swap via Uniswap V3 (raw calldata)
    let amount_out_min = bal_nat * U256::from(999) / U256::from(1000);
    let deadline = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 600;
    println!("\n🔄 Swap USDC nat→USDC.e");

    let call = alloy::rpc::types::TransactionRequest::default()
        .to(UNI_ROUTER)
        .input(calldata);

    let tx = provider.send_transaction(call).await?.watch().await?;
    println!("  ✅ swap: {tx}");
    tokio::time::sleep(Duration::from_secs(5)).await;

    let bal_usdce = usdce.balanceOf(eoa).call().await?;
    println!("USDC.e: {bal_usdce}");
    if bal_usdce.is_zero() { anyhow::bail!("No USDC.e after swap"); }

    // 3. Approve USDC.e → Onramp + Wrap
    println!("\n📝 Approve + Wrap");
    usdce.approve(ONRAMP, bal_usdce).send().await?.watch().await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let onramp = ICollateralOnramp::new(ONRAMP, provider.clone());
    onramp.wrap(USDCE, eoa, bal_usdce).send().await?.watch().await?;
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
    println!("💰 Balance CLOB: raw={raw} → ${bal:.2}");
    if bal > 0.0 { println!("🚀 Listo"); }

    Ok(())
}

fn pad_addr(a: alloy::primitives::Address) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[12..].copy_from_slice(a.as_ref());
    b
}

fn uniswap_calldata(
    token_in: alloy::primitives::Address, token_out: alloy::primitives::Address,
    fee: u32, recipient: alloy::primitives::Address,
    deadline: u64, amount_in: U256, amount_out_min: U256,
) -> Bytes {
    // exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))
    let mut data: Vec<u8> = vec![0x41, 0x4b, 0xf3, 0x89];
    data.extend_from_slice(&pad_addr(token_in));
    data.extend_from_slice(&pad_addr(token_out));
    let mut fb = [0u8; 32]; fb[29..].copy_from_slice(&fee.to_be_bytes()); data.extend_from_slice(&fb);
    data.extend_from_slice(&pad_addr(recipient));
    let mut dl = [0u8; 32]; dl[24..].copy_from_slice(&deadline.to_be_bytes()); data.extend_from_slice(&dl);
    let mut ai = [0u8; 32]; amount_in.to_big_endian(&mut ai); data.extend_from_slice(&ai);
    let mut ao = [0u8; 32]; amount_out_min.to_big_endian(&mut ao); data.extend_from_slice(&ao);
    data.extend_from_slice(&[0u8; 32]); // sqrtPriceLimitX96 = 0
    data.into()
}
