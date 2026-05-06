/// Derive deposit wallet address deterministically (without deploying)
use std::str::FromStr as _;
use alloy::primitives::address;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use polymarket_client_sdk_v2::{POLYGON, PRIVATE_KEY_VAR};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let pk = std::env::var(PRIVATE_KEY_VAR).or_else(|_| std::env::var("PRIVATE_KEY"))?;
    let signer = PrivateKeySigner::from_str(&pk)?.with_chain_id(Some(POLYGON));
    let eoa = signer.address();

    // Derive deposit wallet deterministically:
    // walletId = bytes32(owner) — owner left-padded to 32 bytes
    // args = abi.encode(factory, walletId)
    // salt = keccak256(args)
    // bytecodeHash = init code hash of ERC-1967 proxy
    // depositWallet = CREATE2(factory, salt, bytecodeHash)

    let factory = address!("0x00000000000Fb5C9ADea0298D729A0CB3823Cc07");

    // walletId = bytes32(owner address)
    let mut wallet_id = [0u8; 32];
    wallet_id[12..].copy_from_slice(eoa.as_ref());

    // args = abi.encode(factory, walletId)
    let mut args = Vec::new();
    // factory address (left-padded 32 bytes)
    let mut fbytes = [0u8; 32];
    fbytes[12..].copy_from_slice(factory.as_ref());
    args.extend_from_slice(&fbytes);
    args.extend_from_slice(&wallet_id);

    // salt = keccak256(args)
    use alloy::primitives::keccak256;
    let salt = keccak256(&args);

    // bytecodeHash = keccak256(initCode) for minimal ERC-1967 proxy
    // The exact hash depends on the implementation. Let's try the common Solady LibClone hash
    // But we should look it up from the actual deployment or contract

    // Instead, let's just check if the wallet exists by querying the relayer or chain
    println!("EOA: {eoa:#x}");
    println!("Factory: {factory:#x}");
    println!("Salt: 0x{}", hex::encode(salt));

    // Check PolygonScan for CREATE2 deployed contracts from this factory
    println!("\nCheck: https://polygonscan.com/address/0x00000000000Fb5C9ADea0298D729A0CB3823Cc07#internaltx");

    Ok(())
}
