/// One-shot: aprueba contratos de exchange via relayer de Polymarket para el proxy wallet.
/// Uso: cargo run --example approve_proxy --release
use std::str::FromStr as _;

use alloy::primitives::{Address, address};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer as _;
use anyhow::{Context, Result};
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use reqwest::header;
use serde_json::json;

const RELAYER_URL: &str = "https://relayer-v2.polymarket.com/submit";
const CLOB_URL: &str = "https://clob.polymarket.com";

// Contratos
const PUSD: Address = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");
const CTF_EXCHANGE_V2: Address = address!("0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E");
const NEG_RISK_EXCHANGE: Address = address!("0xC5d563A36AE78145C45a50134d48A1215220f80a");
const CTF_ERC1155: Address = address!("0x4D97DCd97eC945f40cF65F87097ACe5EA0476045");
const NEG_RISK_EXCHANGE_V2: Address = address!("0xe2222d279d744050d28e00520010520000310F59");
const CTF_EXCHANGE_V2_NEW: Address = address!("0xE111180000d2663C0091e4f400237545B87B996B");
const NEG_RISK_ADAPTER: Address = address!("0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296");

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let private_key = std::env::var(PRIVATE_KEY_VAR)
        .or_else(|_| std::env::var("PRIVATE_KEY"))
        .context("Falta POLYMARKET_PRIVATE_KEY")?;

    let relayer_key = std::env::var("RELAYER_API_KEY")
        .context("Falta RELAYER_API_KEY")?;
    let relayer_addr = std::env::var("RELAYER_API_KEY_ADDRESS")
        .context("Falta RELAYER_API_KEY_ADDRESS")?;

    let signer = PrivateKeySigner::from_str(&private_key)?
        .with_chain_id(Some(POLYGON));

    let eoa = signer.address();
    println!("🔑 EOA Magic: {eoa:#x}");

    let proxy = polymarket_client_sdk_v2::derive_proxy_wallet(eoa, POLYGON)
        .context("No se pudo derivar proxy wallet")?;
    println!("🏦 Proxy (funder): {proxy:#x}");

    // Construir calldata para approve ERC-20
    fn erc20_approve_calldata(spender: Address) -> Vec<u8> {
        let mut data = vec![0x09, 0x5e, 0xa7, 0xb3]; // approve(address,uint256)
        let mut spender_bytes = [0u8; 32];
        spender_bytes[12..].copy_from_slice(spender.as_ref());
        data.extend_from_slice(&spender_bytes);
        data.extend_from_slice(&[0xffu8; 32]); // MAX_UINT256
        data
    }

    // Construir calldata para setApprovalForAll ERC-1155
    fn erc1155_approve_calldata(operator: Address) -> Vec<u8> {
        let mut data = vec![0xa2, 0x2c, 0xb4, 0x65]; // setApprovalForAll(address,bool)
        let mut op_bytes = [0u8; 32];
        op_bytes[12..].copy_from_slice(operator.as_ref());
        data.extend_from_slice(&op_bytes);
        data.extend_from_slice(&[0u8; 31]); // bool = true (1 byte)
        data.push(1u8);
        data
    }

    // Pares (nombre, target, calldata)
    let approves: Vec<(&str, Address, Vec<u8>)> = vec![
        ("pUSD → CTF Exchange V2",     PUSD,              erc20_approve_calldata(CTF_EXCHANGE_V2_NEW)),
        ("pUSD → CTF Exchange",        PUSD,              erc20_approve_calldata(CTF_EXCHANGE_V2)),
        ("pUSD → Neg Risk Exchange V2", PUSD,             erc20_approve_calldata(NEG_RISK_EXCHANGE_V2)),
        ("pUSD → Neg Risk Exchange",   PUSD,              erc20_approve_calldata(NEG_RISK_EXCHANGE)),
        ("pUSD → Neg Risk Adapter",    PUSD,              erc20_approve_calldata(NEG_RISK_ADAPTER)),
        ("CTF → CTF Exchange V2",      CTF_ERC1155,       erc1155_approve_calldata(CTF_EXCHANGE_V2_NEW)),
        ("CTF → CTF Exchange",         CTF_ERC1155,       erc1155_approve_calldata(CTF_EXCHANGE_V2)),
        ("CTF → Neg Risk Exchange V2", CTF_ERC1155,       erc1155_approve_calldata(NEG_RISK_EXCHANGE_V2)),
        ("CTF → Neg Risk Exchange",    CTF_ERC1155,       erc1155_approve_calldata(NEG_RISK_EXCHANGE)),
        ("CTF → Neg Risk Adapter",     CTF_ERC1155,       erc1155_approve_calldata(NEG_RISK_ADAPTER)),
    ];

    let client = reqwest::Client::new();

    for (name, target, calldata) in &approves {
        println!("\n📝 {name}");
        let data_hex = alloy::primitives::hex::encode(calldata);
        println!("   target: {target:#x}");
        println!("   data: 0x{data_hex}");

        let body = json!({
            "type": "PROXY",
            "from": format!("{eoa:#x}"),
            "to": format!("{target:#x}"),
            "proxyWallet": format!("{proxy:#x}"),
            "data": format!("0x{data_hex}"),
            "nonce": "0",
            "signature": "0x",
            "signatureParams": {
                "gasPrice": "0",
                "operation": "0",
                "safeTxnGas": "0",
                "baseGas": "0",
                "gasToken": "0x0000000000000000000000000000000000000000",
                "refundReceiver": "0x0000000000000000000000000000000000000000"
            },
        });

        let resp = client
            .post(RELAYER_URL)
            .header("RELAYER_API_KEY", &relayer_key)
            .header("RELAYER_API_KEY_ADDRESS", &relayer_addr)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                let text = r.text().await.unwrap_or_default();
                if status.is_success() {
                    println!("   ✅ status={status}: {text}");
                } else {
                    println!("   ❌ HTTP {status}: {text}");
                    println!("   Body enviado: {body}");
                }
            }
            Err(e) => println!("   ❌ Error de red: {e}"),
        }
    }

    // Verificar balance CLOB
    println!("\n🔗 Conectando al CLOB V2...");
    let clob = Client::new(CLOB_URL, Config::default())?
        .authentication_builder(&signer)
        .signature_type(SignatureType::Proxy)
        .authenticate()
        .await
        .context("Fallo autenticación CLOB")?;

    let ok = clob.ok().await?;
    println!("✅ CLOB OK: {ok}");

    let req = BalanceAllowanceRequest::builder()
        .asset_type(AssetType::Collateral)
        .build();
    clob.update_balance_allowance(req.clone()).await?;

    let b = clob.balance_allowance(req).await?;
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("💰 Balance: raw={raw} → ${bal:.2}");
    println!("   Allowances: {:?}", b.allowances);

    Ok(())
}
