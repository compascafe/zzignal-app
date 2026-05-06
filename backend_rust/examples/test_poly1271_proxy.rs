use std::str::FromStr as _;
use alloy::primitives::address;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType, Side, OrderType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::gamma;
use polymarket_client_sdk_v2::gamma::types::request::MarketsRequest;
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use polymarket_client_sdk_v2::types::Decimal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("EOA: {eoa:#x}");

    let funder = address!("0x0000000000000000000000000000000000000000");
    println!("Funder: {funder:#x}");

    // Get token
    let gm = gamma::Client::default();
    let mkts = gm.markets(&MarketsRequest::builder().closed(false).limit(1).build()).await?;
    let token = &mkts[0].clob_token_ids.as_ref().unwrap()[0];
    println!("Token: {token}");

    // Test 1: Poly1271 + funder=0x059...
    println!("\n─── Poly1271 + funder=0x059... ───");
    let clob = Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .funder(funder)
        .signature_type(SignatureType::Poly1271)
        .authenticate().await?;

    let req = BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build();
    clob.update_balance_allowance(req.clone()).await?;
    let b = clob.balance_allowance(req).await?;
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("  Balance: raw={raw} → ${bal:.2}");
    println!("  Allowances: {:?}", b.allowances);

    // Place test order
    println!("\n  Placing BUY @ 0.01...");
    let order = clob.limit_order()
        .token_id(token.clone())
        .order_type(OrderType::GTC)
        .price(Decimal::from_str("0.01")?)
        .size(Decimal::from_str("5")?)
        .side(Side::Buy)
        .build().await?;
    let signed = clob.sign(&signer, order).await?;

    match clob.post_order(signed).await {
        Ok(r) => println!("  ✅ Order posted: id={}", &r.order_id[..r.order_id.len().min(16)]),
        Err(e) => println!("  ❌ Post order: {e}"),
    }

    Ok(())
}
