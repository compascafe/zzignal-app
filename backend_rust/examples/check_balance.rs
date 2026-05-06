/// Ejemplo: verifica balance CLOB V2 con SignatureType::GnosisSafe
/// Uso: cargo run --example check_balance --release
use std::str::FromStr as _;

use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer as _;
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR, derive_safe_wallet, derive_proxy_wallet};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let private_key = std::env::var(PRIVATE_KEY_VAR)
        .or_else(|_| std::env::var("PRIVATE_KEY"))
        .expect("Falta POLYMARKET_PRIVATE_KEY");

    let signer = PrivateKeySigner::from_str(&private_key)?
        .with_chain_id(Some(POLYGON));

    let eoa = signer.address();
    println!("🔑 EOA: {eoa:#x}");

    let safe_addr = derive_safe_wallet(eoa, POLYGON);
    let proxy_addr = derive_proxy_wallet(eoa, POLYGON);
    println!("🏦 Safe derivada:   {safe_addr:#x?}");
    println!("🏦 Proxy derivada:  {proxy_addr:#x?}");

    // ── Probar GnosisSafe ──────────────────────────────────
    println!("\n─── Probando SignatureType::GnosisSafe ───");
    match test_balance(&signer, SignatureType::GnosisSafe).await {
        Ok(bal) => println!("✅ BALANCE Safe: {bal}"),
        Err(e) => println!("❌ Error Safe: {e}"),
    }

    // ── Probar Proxy ───────────────────────────────────────
    println!("\n─── Probando SignatureType::Proxy ───");
    match test_balance(&signer, SignatureType::Proxy).await {
        Ok(bal) => println!("✅ BALANCE Proxy: {bal}"),
        Err(e) => println!("❌ Error Proxy: {e}"),
    }

    // ── Probar EOA ─────────────────────────────────────────
    println!("\n─── Probando SignatureType::Eoa ───");
    match test_balance(&signer, SignatureType::Eoa).await {
        Ok(bal) => println!("✅ BALANCE EOA: {bal}"),
        Err(e) => println!("❌ Error EOA: {e}"),
    }

    Ok(())
}

async fn test_balance(signer: &PrivateKeySigner, st: SignatureType) -> anyhow::Result<String> {
    let client = Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(signer)
        .signature_type(st)
        .authenticate()
        .await?;

    let req = BalanceAllowanceRequest::builder()
        .asset_type(AssetType::Collateral)
        .build();

    client.update_balance_allowance(req.clone()).await?;

    let b = client.balance_allowance(req).await?;
    Ok(format!("raw={} | allowances: {}", b.balance, serde_json::to_string(&b.allowances).unwrap_or_default()))
}
