/// Regenera CLOB API keys y verifica balance con proxy 0x059...
/// Uso: cargo run --example regen_keys --release
use std::str::FromStr as _;

use alloy::primitives::address;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer as _;
use anyhow::{Context, Result};
use polymarket_client_sdk_v2::auth::Credentials;
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use secrecy::ExposeSecret;

const CLOB_URL: &str = "https://clob.polymarket.com";
const PROXY: alloy::primitives::Address = address!("0x0000000000000000000000000000000000000000");

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let private_key = std::env::var(PRIVATE_KEY_VAR)
        .or_else(|_| std::env::var("PRIVATE_KEY"))
        .context("Falta POLYMARKET_PRIVATE_KEY")?;

    let signer = PrivateKeySigner::from_str(&private_key)?
        .with_chain_id(Some(POLYGON));

    let eoa = signer.address();
    println!("🔑 EOA: {eoa:#x}");

    // 1. Crear cliente sin autenticar y eliminar keys viejas
    let unauth = Client::new(CLOB_URL, Config::default())?;
    
    println!("\n📋 API keys existentes:");
    match unauth.derive_api_key(&signer, None).await {
        Ok(creds) => {
            println!("   Key: {}", creds.key());
            println!("🗑️  Eliminando keys viejas...");
            // Autenticamos temporalmente para borrar
            let temp = Client::new(CLOB_URL, Config::default())?
                .authentication_builder(&signer)
                .credentials(creds)
                .authenticate()
                .await?;
            match temp.delete_api_key().await {
                Ok(v) => println!("   ✅ Keys eliminadas: {v}"),
                Err(e) => println!("   ⚠️  Error eliminando: {e}"),
            }
        }
        Err(_) => println!("   No hay keys existentes"),
    }

    // 2. Crear nuevas API keys
    println!("\n🔐 Creando nuevas API keys...");
    let new_creds = Client::new(CLOB_URL, Config::default())?
        .create_api_key(&signer, None)
        .await
        .context("No se pudo crear API key")?;

    let api_key = new_creds.key().to_string();
    let api_secret = new_creds.secret().expose_secret().to_string();
    let api_passphrase = new_creds.passphrase().expose_secret().to_string();

    println!("   CLOB_API_KEY={api_key}");
    println!("   CLOB_API_SECRET={api_secret}");
    println!("   CLOB_API_PASSPHRASE={api_passphrase}");
    println!("\n📋 Actualiza tu .env con estos valores");

    // 3. Autenticar con Proxy + nuevas keys
    println!("\n🔗 Conectando al CLOB con Proxy 0x059...");
    let clob = Client::new(CLOB_URL, Config::default())?
        .authentication_builder(&signer)
        .credentials(new_creds)
        .funder(PROXY)
        .signature_type(SignatureType::Proxy)
        .authenticate()
        .await
        .context("Fallo autenticación")?;

    let ok = clob.ok().await?;
    println!("✅ CLOB OK: {ok}");

    // 4. Verificar balance
    let req = BalanceAllowanceRequest::builder()
        .asset_type(AssetType::Collateral)
        .build();
    clob.update_balance_allowance(req.clone()).await?;
    let b = clob.balance_allowance(req).await?;
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("💰 Balance: raw={raw} → ${bal:.2}");
    println!("   Allowances: {:?}", b.allowances);

    if bal > 0.0 {
        println!("\n🎉 FUNCIONA — actualiza .env con las nuevas keys y despliega");
    } else {
        println!("\n⚠️  Balance sigue 0. El proxy probablemente necesita approves on-chain.");
    }

    Ok(())
}
