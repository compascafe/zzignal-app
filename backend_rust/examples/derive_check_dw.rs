/// Derive deposit wallet and check if it exists on-chain
use std::str::FromStr as _;
use alloy::primitives::{address, keccak256, Address, B256};

use alloy::providers::ProviderBuilder;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};

const FACTORY: Address = address!("0x00000000000Fb5C9ADea0298D729A0CB3823Cc07");
const BYTECODE_HASH: B256 = b256!("0x21c3a0680b7435f26e73f8caacc4ffed6e2f5a45c9f02cdfe8faf8400e7c0f00"); // ERC-1967 proxy init code hash for Solady LibClone

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address a) external view returns (uint256);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();
    println!("EOA: {eoa:#x}");

    // walletId = bytes32(owner)
    let mut wallet_id = [0u8; 32];
    wallet_id[12..].copy_from_slice(eoa.as_ref());

    // args = abi.encode(address, bytes32) = factory (32B) + walletId (32B)
    let mut args = [0u8; 64];
    args[12..32].copy_from_slice(FACTORY.as_ref());
    args[32..].copy_from_slice(&wallet_id);
    let salt = keccak256(args);

    // depositWallet = CREATE2(factory, salt, bytecodeHash)
    let init_code_hash = BYTECODE_HASH;
    let mut preimage = [0u8; 1 + 32 + 32];
    preimage[0] = 0xff;
    preimage[1..21].copy_from_slice(FACTORY.as_ref());
    preimage[21..53].copy_from_slice(salt.as_ref());
    preimage[53..].copy_from_slice(init_code_hash.as_ref());
    let dw_hash = keccak256(preimage);
    let dw: Address = Address::from_slice(&dw_hash[12..]);

    println!("Deposit wallet: {dw:#x}");
    println!("Salt: {salt:#x}");

    // Check if wallet exists (has code or balance)
    let provider = ProviderBuilder::new().wallet(signer).connect("https://polygon-bor-rpc.publicnode.com").await?;
    let code = provider.get_code_at(dw).await?;
    println!("Has code: {}", !code.is_empty());

    // Check pUSD balance
    let pusd = IERC20::new(address!("0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB"), provider);
    let bal = pusd.balanceOf(dw).call().await?;
    println!("pUSD balance: {bal}");

    // Also check on PolygonScan
    println!("\nPolygonScan: https://polygonscan.com/address/{dw:#x}");

    Ok(())
}
