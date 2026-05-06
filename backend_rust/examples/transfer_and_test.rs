use std::str::FromStr as _;
use std::time::Duration;
use alloy::primitives::{address, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType, Side, OrderType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use polymarket_client_sdk_v2::types::Decimal;
use polymarket_client_sdk_v2::gamma;
use polymarket_client_sdk_v2::gamma::types::request::MarketsRequest;

const PUSD: alloy::primitives::Address = address!("0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB");
const PROXY: alloy::primitives::Address = address!("0x0000000000000000000000000000000000000000");

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function transfer(address to, uint256 value) external returns (bool);
        function balanceOf(address a) external view returns (uint256);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("EOA: {eoa:#x}");
    println!("Funder: {PROXY:#x}");

    let provider = ProviderBuilder::new().wallet(signer.clone()).connect("https://polygon-bor-rpc.publicnode.com").await?;
    let pusd = IERC20::new(PUSD, provider);

    // Transfer pUSD from EOA to proxy
    let bal_eoa = pusd.balanceOf(eoa).call().await?;
    println!("\npUSD EOA: {bal_eoa}");
    if bal_eoa > U256::ZERO {
        println!("Transferring to proxy...");
        pusd.transfer(PROXY, bal_eoa).send().await?.watch().await?;
        println!("  ✅");
        tokio::time::sleep(Duration::from_secs(5)).await;
        let bal_proxy = pusd.balanceOf(PROXY).call().await?;
        println!("pUSD proxy: {bal_proxy}");
    }

    // Test order with Poly1271
    let gm = gamma::Client::default();
    let mkts = gm.markets(&MarketsRequest::builder().closed(false).limit(1).build()).await?;
    let token = &mkts[0].clob_token_ids.as_ref().unwrap()[0];

    println!("\n─── Poly1271 + proxy ───");
    let clob = Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .funder(PROXY)
        .signature_type(SignatureType::Poly1271)
        .authenticate().await?;

    let req = BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build();
    clob.update_balance_allowance(req.clone()).await?;
    let b = clob.balance_allowance(req).await?;
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("Balance: raw={raw} → ${bal:.2}");

    println!("Placing test order...");
    let order = clob.limit_order()
        .token_id(token.clone()).order_type(OrderType::GTC)
        .price(Decimal::from_str("0.01")?).size(Decimal::from_str("5")?)
        .side(Side::Buy).build().await?;
    let signed = clob.sign(&signer, order).await?;
    match clob.post_order(signed).await {
        Ok(r) => {
            println!("✅ TEST ORDER OK: id={}", &r.order_id[..r.order_id.len().min(16)]);
            // Cancel it immediately
            clob.cancel_order(&r.order_id).await?;
            println!("✅ Cancelled");
        }
        Err(e) => println!("❌ {e}"),
    }

    Ok(())
}
