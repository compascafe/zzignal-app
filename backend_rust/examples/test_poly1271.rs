/// Test: does Poly1271 with EOA as funder work for placing orders?
use std::str::FromStr as _;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("EOA: {eoa:#x}");

    // Try Poly1271 with EOA as funder
    println!("\n─── Poly1271 + funder=EOA ───");
    match Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .funder(eoa)
        .signature_type(SignatureType::Poly1271)
        .authenticate().await
    {
        Ok(clob) => {
            let req = BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build();
            clob.update_balance_allowance(req.clone()).await?;
            let b = clob.balance_allowance(req).await?;
            let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
            println!("  Balance: raw={raw} → ${:.2}", if raw > 1_000.0 { raw / 1_000_000.0 } else { raw });
            println!("  Allowances: {:?}", b.allowances);

            // Try placing a test order
            println!("\n  Trying limit order...");
            match clob.limit_order()
                .token_id(polymarket_client_sdk_v2::types::U256::from(1u64))
                .price("0.01".parse()?)
                .size("5".parse()?)
                .side(polymarket_client_sdk_v2::clob::types::Side::Buy)
                .build().await
            {
                Ok(order) => {
                    match clob.sign(&signer, order).await {
                        Ok(signed) => {
                            match clob.post_order(signed).await {
                                Ok(resp) => println!("  ✅ Order placed: {:?}", resp),
                                Err(e) => println!("  ❌ Post order: {e}"),
                            }
                        }
                        Err(e) => println!("  ❌ Sign: {e}"),
                    }
                }
                Err(e) => println!("  ❌ Build: {e}"),
            }
        }
        Err(e) => println!("  ❌ Auth: {e}"),
    }

    Ok(())
}
