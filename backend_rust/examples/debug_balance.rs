/// Debug: llamada HTTP directa al CLOB balance-allowance con L2 auth
/// Uso: cargo run --example debug_balance --release
use std::str::FromStr as _;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy::primitives::address;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer as _;
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE};
use hmac::{Hmac, Mac};
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use secrecy::ExposeSecret;
use sha2::Sha256;

const CLOB_URL: &str = "https://clob.polymarket.com";
const PROXY: alloy::primitives::Address = address!("0x0000000000000000000000000000000000000000");

type HmacSha256 = Hmac<Sha256>;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let private_key = std::env::var(PRIVATE_KEY_VAR)
        .or_else(|_| std::env::var("PRIVATE_KEY"))
        .context("Falta POLYMARKET_PRIVATE_KEY")?;

    let api_key = std::env::var("CLOB_API_KEY").context("Falta CLOB_API_KEY")?;
    let api_secret = std::env::var("CLOB_API_SECRET").context("Falta CLOB_API_SECRET")?;
    let api_passphrase = std::env::var("CLOB_API_PASSPHRASE").context("Falta CLOB_API_PASSPHRASE")?;

    let signer = PrivateKeySigner::from_str(&private_key)?
        .with_chain_id(Some(POLYGON));
    let eoa = signer.address();

    println!("🔑 EOA: {eoa:#x}");
    println!("🏦 Funder: {PROXY:#x}");

    // ─── 1. Via SDK ──────────────────────────────────────
    println!("\n─── Vía SDK ───");
    let clob = Client::new(CLOB_URL, Config::default())?
        .authentication_builder(&signer)
        .funder(PROXY)
        .signature_type(SignatureType::Proxy)
        .authenticate()
        .await?;

    let req = BalanceAllowanceRequest::builder()
        .asset_type(AssetType::Collateral)
        .build();
    clob.update_balance_allowance(req.clone()).await?;
    let b = clob.balance_allowance(req).await?;
    println!("SDK raw balance: {}", b.balance);
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("SDK balance: ${bal:.2}");
    println!("SDK allowances: {:?}", b.allowances);

    // ─── 2. HTTP directo ─────────────────────────────────
    println!("\n─── HTTP Directo ───");
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let path = "/balance-allowance?asset_type=COLLATERAL&signature_type=1";
    let message = format!("{timestamp}GET{path}");
    println!("Message: {message}");

    // HMAC-SHA256 signature
    let decoded = URL_SAFE.decode(&api_secret)?;
    let mut mac = HmacSha256::new_from_slice(&decoded)?;
    mac.update(message.as_bytes());
    let sig = URL_SAFE.encode(mac.finalize().into_bytes());
    println!("Signature: {sig}");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{CLOB_URL}{path}"))
        .header("POLY_ADDRESS", format!("{eoa:#x}"))
        .header("POLY_API_KEY", &api_key)
        .header("POLY_PASSPHRASE", &api_passphrase)
        .header("POLY_SIGNATURE", &sig)
        .header("POLY_TIMESTAMP", timestamp.to_string())
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await?;
    println!("HTTP {status}:");
    println!("{body}");

    // ─── 3. HTTP con signature_type=2 (Proxy) ────────────
    println!("\n─── HTTP signature_type=2 (Proxy) ───");
    let path2 = "/balance-allowance?asset_type=COLLATERAL&signature_type=2";
    let message2 = format!("{timestamp}GET{path2}");
    let mut mac2 = HmacSha256::new_from_slice(&decoded)?;
    mac2.update(message2.as_bytes());
    let sig2 = URL_SAFE.encode(mac2.finalize().into_bytes());

    let resp2 = client
        .get(format!("{CLOB_URL}{path2}"))
        .header("POLY_ADDRESS", format!("{eoa:#x}"))
        .header("POLY_API_KEY", &api_key)
        .header("POLY_PASSPHRASE", &api_passphrase)
        .header("POLY_SIGNATURE", &sig2)
        .header("POLY_TIMESTAMP", timestamp.to_string())
        .send()
        .await?;

    let status2 = resp2.status();
    let body2 = resp2.text().await?;
    println!("HTTP {status2}:");
    println!("{body2}");

    Ok(())
}
