/// Deploy deposit wallet + transfer pUSD + test order with Poly1271
use std::str::FromStr as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use alloy::primitives::{address, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use anyhow::{Context, Result};
use polymarket_client_sdk_v2::clob::types::{AssetType, SignatureType};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use serde_json::json;

const RPC: &str = "https://polygon-bor-rpc.publicnode.com";
const CLOB_URL: &str = "https://clob.polymarket.com";
const RELAYER: &str = "https://relayer-v2.polymarket.com";
const DEPOSIT_FACTORY: alloy::primitives::Address = address!("0x00000000000Fb5C9ADea0298D729A0CB3823Cc07");
const PUSD: alloy::primitives::Address = address!("0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB");

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function transfer(address to, uint256 value) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))
        .context("Missing POLYMARKET_PRIVATE_KEY")?;
    let relayer_key = std::env::var("RELAYER_API_KEY").unwrap_or_default();
    let relayer_addr = std::env::var("RELAYER_API_KEY_ADDRESS").unwrap_or_default();

    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("🔑 EOA: {eoa:#x}");

    let provider = ProviderBuilder::new().wallet(signer.clone()).connect(RPC).await?;
    let http = reqwest::Client::new();

    // ─── 1. Deploy deposit wallet via relayer ────────
    println!("\n─── 1. Deploying deposit wallet ───");
    let deploy_body = json!({
        "type": "WALLET-CREATE",
        "from": format!("{eoa:#x}"),
        "to": format!("{DEPOSIT_FACTORY:#x}"),
    });
    println!("  POST /submit WALLET-CREATE");

    let resp = http.post(format!("{RELAYER}/submit"))
        .header("RELAYER_API_KEY", &relayer_key)
        .header("RELAYER_API_KEY_ADDRESS", &relayer_addr)
        .header("Content-Type", "application/json")
        .json(&deploy_body)
        .send().await?;

    let status = resp.status();
    let body = resp.text().await?;
    println!("  HTTP {status}: {body}");

    if !status.is_success() {
        anyhow::bail!("Deploy failed: {body}");
    }

    let deploy_resp: serde_json::Value = serde_json::from_str(&body)?;
    let tx_id = deploy_resp["transactionID"].as_str().context("No transactionID")?;
    println!("  transactionID: {tx_id}");

    // Poll for completion
    let deposit_wallet = loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let resp = http.get(format!("{RELAYER}/transaction/{tx_id}"))
            .header("RELAYER_API_KEY", &relayer_key)
            .header("RELAYER_API_KEY_ADDRESS", &relayer_addr)
            .send().await?;
        let body = resp.text().await?;
        let tx: serde_json::Value = serde_json::from_str(&body)?;
        let state = tx["state"].as_str().unwrap_or("");
        println!("  State: {state}");

        if state == "STATE_CONFIRMED" || state == "STATE_MINED" {
            // Extract deposit wallet from WalletDeployed event or proxyAddress field
            if let Some(addr) = tx["proxyAddress"].as_str() {
                break addr.to_string();
            }
            if let Some(logs) = tx["logs"].as_array() {
                for log in logs {
                    if let Some(addr) = log["address"].as_str() {
                        // The deployed wallet might be in topics or data
                    }
                }
            }
            // Fallback: derive deterministically
            // walletId = bytes32(owner), salt = keccak256(abi.encode(factory, walletId))
            // depositWallet = CREATE2 factory, salt, bytecodeHash
            anyhow::bail!("Cannot determine deposit wallet from response: {body}");
        }
        if state.contains("FAILED") || state.contains("INVALID") {
            anyhow::bail!("Deploy failed: {body}");
        }
    };

    println!("  Deposit wallet: {deposit_wallet}");
    let dw_addr: alloy::primitives::Address = deposit_wallet.parse()?;

    // ─── 2. Transfer pUSD from EOA to deposit wallet ────
    println!("\n─── 2. Transfer pUSD EOA → deposit wallet ───");
    let pusd = IERC20::new(PUSD, provider.clone());
    let bal = pusd.balanceOf(eoa).call().await?;
    println!("  pUSD balance EOA: {bal}");

    if bal > U256::ZERO {
        println!("  Transferring {bal} pUSD...");
        pusd.transfer(dw_addr, bal).send().await
            .map_err(|e| anyhow::anyhow!("transfer: {e}"))?
            .watch().await?;
        println!("  ✅");
        tokio::time::sleep(Duration::from_secs(5)).await;

        let dw_bal = pusd.balanceOf(dw_addr).call().await?;
        println!("  pUSD balance DW: {dw_bal}");
    }

    // ─── 3. Test order with Poly1271 ────
    println!("\n─── 3. Testing order with Poly1271 ───");
    let clob = Client::new(CLOB_URL, Config::default())?
        .authentication_builder(&signer)
        .funder(dw_addr)
        .signature_type(SignatureType::Poly1271)
        .authenticate().await
        .context("Auth failed")?;

    // Update balance
    let req = BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).build();
    clob.update_balance_allowance(req.clone()).await?;
    let b = clob.balance_allowance(req).await?;
    let raw: f64 = b.balance.to_string().parse().unwrap_or(0.0);
    let bal = if raw > 1_000.0 { raw / 1_000_000.0 } else { raw };
    println!("  Balance DW: raw={raw} → ${bal:.2}");

    Ok(())
}
