/// Test: authenticate WITHOUT explicit .credentials() but WITH .funder() set BEFORE .authenticate()
/// This should trigger create_or_derive_api_key WITH the proxy context.
use std::str::FromStr as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer as _;
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::types::address;
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    println!("🔑 EOA: {:#x}", signer.address());

    let proxy = address!("0x0000000000000000000000000000000000000000");

    // ── Test 1: SIN credentials, CON funder → keys se crean con contexto proxy ──
    println!("\n─── Test 1: authenticate SIN credentials, CON funder ───");
    match Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .funder(proxy)
        .signature_type(SignatureType::Proxy)
        .authenticate()
        .await
    {
        Ok(clob) => {
            let keys = clob.api_keys().await?;
            println!("  API keys: {keys:?}");
            let b = clob.balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Proxy)
                    .build()
            ).await?;
            println!("  Balance: raw={} | allowances: {:?}", b.balance, b.allowances);
        }
        Err(e) => println!("  ❌ Auth failed: {e}"),
    }

    // ── Test 2: CON credentials del .env, CON funder ──
    println!("\n─── Test 2: authenticate CON creds del .env, CON funder ───");
    let api_key = std::env::var("CLOB_API_KEY")?;
    let api_secret = std::env::var("CLOB_API_SECRET")?;
    let api_passphrase = std::env::var("CLOB_API_PASSPHRASE")?;
    let creds = polymarket_client_sdk_v2::auth::Credentials::new(
        polymarket_client_sdk_v2::auth::Uuid::parse_str(&api_key)?,
        api_secret, api_passphrase
    );
    match Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .credentials(creds)
        .funder(proxy)
        .signature_type(SignatureType::Proxy)
        .authenticate()
        .await
    {
        Ok(clob) => {
            let b = clob.balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Proxy)
                    .build()
            ).await?;
            println!("  Balance: raw={} | allowances: {:?}", b.balance, b.allowances);
        }
        Err(e) => println!("  ❌ Auth failed: {e}"),
    }

    // ── Test 3: SIN creds, SIN funder (EOA default) ──
    println!("\n─── Test 3: EOA default ───");
    match Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .authenticate()
        .await
    {
        Ok(clob) => {
            let b = clob.balance_allowance(
                BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build()
            ).await?;
            println!("  Balance: raw={} | allowances: {:?}", b.balance, b.allowances);
        }
        Err(e) => println!("  ❌ Auth failed: {e}"),
    }

    Ok(())
}
