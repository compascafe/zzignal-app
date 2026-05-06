use std::str::FromStr as _;
use alloy::primitives::{address,U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use polymarket_client_sdk_v2::{POLYGON,PRIVATE_KEY_VAR};

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address a) external view returns (uint256);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    let p = ProviderBuilder::new().wallet(signer).connect("https://polygon-bor-rpc.publicnode.com").await?;

    // Verificar balances
    let usdcn = IERC20::new(address!("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359"), p.clone());
    let usdce = IERC20::new(address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"), p.clone());
    println!("USDC nat: {}", usdcn.balanceOf(eoa).call().await?);
    println!("USDC.e:   {}", usdce.balanceOf(eoa).call().await?);

    // Ver si approve ya está (allowance)
    let router = address!("0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45");
    // read allowance via raw call
    Ok(())
}
