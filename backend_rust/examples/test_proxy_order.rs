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

    // Get a real BTC market token
    let gm = gamma::Client::default();
    let mkts = gm.markets(&MarketsRequest::builder()
        .closed(false).limit(1).build()).await?;
    let mkt = &mkts[0];
    let tokens = mkt.clob_token_ids.as_ref().unwrap();
    let token_id_up = &tokens[0];
    println!("Market: {}", mkt.question.as_deref().unwrap_or("?"));
    println!("Token UP: {token_id_up}");

    // ── Test Proxy + funder=0x059... ──
    println!("\n─── Proxy + funder 0x059... ───");
    let proxy = address!("0x0000000000000000000000000000000000000000");
    let clob_p = Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .funder(proxy)
        .signature_type(SignatureType::Proxy)
        .authenticate().await?;
    
    let bal_p = clob_p.balance_allowance(
        BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build()
    ).await?;
    println!("  Balance: raw={}", bal_p.balance);

    // Try order
    println!("  Placing BUY order...");
    let order = clob_p.limit_order()
        .token_id(token_id_up.clone())
        .order_type(OrderType::GTC)
        .price(Decimal::from_str("0.01")?)
        .size(Decimal::from_str("5")?)
        .side(Side::Buy)
        .build().await?;
    let signed = clob_p.sign(&signer, order).await?;
    match clob_p.post_order(signed).await {
        Ok(r) => println!("  ✅ Order: id={}", &r.order_id[..r.order_id.len().min(16)]),
        Err(e) => println!("  ❌ {e}"),
    }

    Ok(())
}
