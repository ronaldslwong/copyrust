use crate::send_tx::block_razor::{
    initialize_blockrazor_client, send_tx_blockrazor, get_blockrazor_health,
    create_instruction_blockrazor, blockrazor_tip
};
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration
    let blockrazor_endpoint = "http://frankfurt.solana-grpc.blockrazor.xyz:80";
    let auth_key = "your_auth_key_here"; // Replace with your actual auth key
    let mainnet_rpc = "https://api.mainnet-beta.solana.com"; // Replace with your RPC endpoint
    
    // Initialize BlockRazor client
    println!("Initializing BlockRazor client...");
    initialize_blockrazor_client(blockrazor_endpoint, auth_key, true).await;
    
    // Test health check
    println!("Testing health check...");
    match get_blockrazor_health(auth_key).await {
        Ok(status) => println!("Health status: {}", status),
        Err(e) => println!("Health check failed: {:?}", e),
    }
    
    // Example: Send a simple transfer transaction
    println!("Building and sending transaction...");
    let keypair = get_wallet_keypair();
    let receiver = Pubkey::from_str("11111111111111111111111111111112")?; // Example receiver
    
    // Create a simple transfer instruction
    let transfer_ix = system_instruction::transfer(
        &keypair.pubkey(),
        &receiver,
        1000, // 0.000001 SOL
    );
    
    // Build instructions with BlockRazor compute budget and tip
    let instructions = create_instruction_blockrazor(
        vec![transfer_ix],
        1_000_000, // 0.001 SOL tip
        1000,      // Compute unit price
        get_nonce_account(),
    );
    
    // Get recent blockhash
    let rpc_client = RpcClient::new_with_commitment(
        mainnet_rpc.to_string(),
        CommitmentConfig::confirmed(),
    );
    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    
    // Build and sign transaction
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        recent_blockhash,
    );
    
    // Send transaction via BlockRazor
    match send_tx_blockrazor(
        &transaction,
        auth_key,
        "fast",           // Mode: "fast" or "sandwichMitigation"
        Some(3),          // Safe window (only for sandwichMitigation mode)
        false,            // Revert protection
    ).await {
        Ok(signature) => println!("Transaction sent successfully! Signature: {}", signature),
        Err(e) => println!("Failed to send transaction: {:?}", e),
    }
    
    Ok(())
}

/// Example function showing how to create a tip instruction manually
pub fn example_tip_instruction() -> Instruction {
    let keypair = get_wallet_keypair();
    let tip_amount = 1_000_000; // 0.001 SOL
    
    // Use one of the BlockRazor tip accounts
    let tip_account = "FjmZZrFvhnqqb9ThCuMVnENaM3JGVuGWNyCAxRJcFpg9";
    
    blockrazor_tip(tip_account, tip_amount, &keypair.pubkey())
} 