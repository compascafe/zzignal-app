use std::str::FromStr as _;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType, Side};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::gamma;
use polymarket_client_sdk_v2::gamma::types::request::MarketsRequest;
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("EOA: {eoa:#x}");

    // Get a real market token
    println!("Fetching real market...");
    let gm = gamma::Client::default();
    let mkt = gm.markets(&MarketsRequest::builder()
        .active(true).closed(false).tag("btc").limit(1).build()).await?;
    let token = &mkt[0].clob_token_ids.as_ref().unwrap()[0];
    println!("Token: {token}");

    // Test 1: EOA (current, fails with 400)
    println!("\n─── Test 1: SignatureType::Eoa ───");
    let clob_eoa = Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer).authenticate().await?;
    let bal_eoa = clob_eoa.balance_allowance(BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build()).await?;
    println!("  Balance: raw={}", bal_eoa.balance);

    println!("  Placing order...");
    match clob_eoa.limit_order()
        .token_id(token.parse::<polymarket_client_sdk_v2::types::U256>()?)
        .price("0.01".parse()?).size("5".parse()?).side(Side::Buy)
        .build().await?.post(&signer).await
    {
        Ok(r) => println!("  ✅ {r:?}"),
        Err(e) => println!("  ❌ {e}"),
    }

    // Test 2: Poly1271 + EOA as funder
    println!("\n─── Test 2: SignatureType::Poly1271 + funder=EOA ───");
    let clob_p = Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .funder(eoa)
        .signature_type(SignatureType::Poly1271)
        .authenticate().await?;
    let bal_p = clob_p.balance_allowance(BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build()).await?;
    println!("  Balance: raw={}", bal_p.balance);

    println!("  Placing order...");
    match clob_p.limit_order()
        .token_id(token.parse::<polymarket_client_sdk_v2::types::U256>()?)
        .price("0.01".parse()?).size("5".parse()?).side(Side::Buy)
        .build().await?.post(&signer).await
    {
        Ok(r) => println!("  ✅ {r:?}"),
        Err(e) => println!("  ❌ {e}"),
    }

    Ok(())
}
