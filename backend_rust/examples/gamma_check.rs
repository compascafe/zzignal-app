use std::str::FromStr as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer as _;
use polymarket_client_sdk_v2::gamma;
use polymarket_client_sdk_v2::gamma::types::request::PublicProfileRequest;
use polymarket_client_sdk_v2::clob::types::SignatureType;
use polymarket_client_sdk_v2::clob::{Client,Config};
use polymarket_client_sdk_v2::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk_v2::clob::types::AssetType;
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};
use polymarket_client_sdk_v2::types::address;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("EOA: {eoa:#x}");

    let gm = gamma::Client::default();
    let req = PublicProfileRequest::builder().address(eoa).build();
    let profile = gm.public_profile(&req).await?;
    println!("Gamma public_profile:");
    println!("  proxy_wallet: {:?}", profile.proxy_wallet.map(|a| format!("{a:#x}")));
    println!("  username: {:?}", profile.x_username);

    // Ver si tiene multiple profiles o wallets
    let proxy = address!("0x0000000000000000000000000000000000000000");
    let req2 = PublicProfileRequest::builder().address(proxy).build();
    match gm.public_profile(&req2).await {
        Ok(p2) => {
            println!("\nGamma profile de 0x059...:");
            println!("  proxy_wallet: {:?}", p2.proxy_wallet.map(|a| format!("{a:#x}")));
            println!("  username: {:?}", p2.x_username);
        }
        Err(e) => println!("\nGamma profile 0x059... error: {e}"),
    }

    Ok(())
}
