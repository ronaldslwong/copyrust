use std::str::FromStr;

use bs58;
use base64::{Engine as _, engine::general_purpose};
use reqwest::Client;
use rand::seq::SliceRandom;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use solana_sdk::instruction::Instruction;
use crate::config_load::GLOBAL_CONFIG;
use crate::init::wallet_loader::get_wallet_keypair;
use once_cell::sync::Lazy;

// List of ZeroSlot tip accounts
static ZEROSLOT_TIP_ACCOUNTS: &[&str] = &[
    "4HiwLEP2Bzqj3hM2ENxJuzhcPCdsafwiet3oGkMkuQY4",
    "7toBU3inhmrARGngC7z6SjyP85HgGMmCTEwGNRAcYnEK",
    "8mR3wB1nh4D6J9RUCugxUpc6ya8w38LPxZ3ZjcBhgzws",
    "6SiVU5WEwqfFapRuYCndomztEwDjvS5xgtEof3PLEGm9",
    "TpdxgNJBWZRL8UXF5mrEsyWxDWx9HQexA9P1eTWQ42p",
    "D8f3WkQu6dCF33cZxuAsrKHrGsqGP2yvAHf8mX6RXnwf",
    "GQPFicsy3P3NXxB5piJohoxACqTvWE9fKpLgdsMduoHE",
    "Ey2JEr8hDkgN8qKJGrLf2yFjRhW7rab99HVxwi5rcvJE",
    "4iUgjMT8q2hNZnLuhpqZ1QtiV8deFPy2ajvvjEpKKgsS",
    "3Rz8uD83QsU8wKvZbgWAPvCNDU6Fy8TSZTMcPm3RB6zt",
];

// Global HTTP client with connection pooling for better performance
static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .pool_max_idle_per_host(50) // Keep up to 50 idle connections per host for high throughput
        .pool_idle_timeout(std::time::Duration::from_secs(120)) // Keep connections alive for 2 minutes
        .tcp_keepalive(Some(std::time::Duration::from_secs(30))) // Enable TCP keep-alive
        .timeout(std::time::Duration::from_secs(3)) // 3 second timeout for larger transactions
        .connect_timeout(std::time::Duration::from_millis(500)) // 500ms connect timeout
        // Remove HTTP/2 prior knowledge to avoid frame size issues with large transactions
        .build()
        .expect("Failed to create HTTP client")
});

async fn send_solana_transaction(
    api_key: &str,
    private_key: &str,
    tip_key: &str,
    to_public_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create a reqwest client

    // Create an RPC client for fetching the latest blockhash
    let connection_for_blockhash = RpcClient::new("https://api.mainnet-beta.solana.com".to_string());

    // Fetch the latest blockhash from the Solana network
    let blockhash = connection_for_blockhash
        .get_latest_blockhash()
        .expect("Failed to get latest blockhash");

    // Decode the sender's private key from a base58-encoded string and create a Keypair object
    let private_key_bytes = bs58::decode(private_key).into_vec().unwrap();
    let sender = Keypair::from_bytes(&private_key_bytes).unwrap();

    // Create PublicKey objects for the receiver and the tip receiver
    let receiver = Pubkey::from_str(to_public_key).unwrap();
    let tip_receiver = Pubkey::from_str(tip_key).unwrap();

    // Create transfer instructions for the main transfer and the tip transfer
    let main_transfer_instruction = system_instruction::transfer(
        &sender.pubkey(), // Sender's public key
        &receiver,        // Receiver's public key
        1,                // Amount to transfer (1 lamports)
    );
    // You need to transfer an amount greater than or equal to 0.001 SOL to any of the following accounts:
    // 4HiwLEP2Bzqj3hM2ENxJuzhcPCdsafwiet3oGkMkuQY4
    // 7toBU3inhmrARGngC7z6SjyP85HgGMmCTEwGNRAcYnEK
    // 8mR3wB1nh4D6J9RUCugxUpc6ya8w38LPxZ3ZjcBhgzws
    // 6SiVU5WEwqfFapRuYCndomztEwDjvS5xgtEof3PLEGm9
    // TpdxgNJBWZRL8UXF5mrEsyWxDWx9HQexA9P1eTWQ42p
    // D8f3WkQu6dCF33cZxuAsrKHrGsqGP2yvAHf8mX6RXnwf
    // GQPFicsy3P3NXxB5piJohoxACqTvWE9fKpLgdsMduoHE
    // Ey2JEr8hDkgN8qKJGrLf2yFjRhW7rab99HVxwi5rcvJE
    // 4iUgjMT8q2hNZnLuhpqZ1QtiV8deFPy2ajvvjEpKKgsS
    // 3Rz8uD83QsU8wKvZbgWAPvCNDU6Fy8TSZTMcPm3RB6zt
    let tip_transfer_instruction = system_instruction::transfer(
        &sender.pubkey(), // Sender's public key
        &tip_receiver,    // Tip receiver's public key
        1000000,           // Amount to transfer as a tip (0.001 SOL in this case)
    );

    // Create a transaction containing the instructions
    let mut transaction = Transaction::new_with_payer(
        &[main_transfer_instruction, tip_transfer_instruction],
        Some(&sender.pubkey()),
    );

    // Sign the transaction with the sender's keypair
    transaction.try_sign(&[&sender], blockhash).expect("Failed to sign transaction");

    Ok(())
}


pub fn zeroslot_tip(tip: u64, from_pubkey: &Pubkey) -> Instruction {
    // Randomly select a tip account from the list
    let tip_account = ZEROSLOT_TIP_ACCOUNTS
        .choose(&mut rand::thread_rng())
        .expect("Failed to select random tip account");
    
    let tip_pubkey = Pubkey::from_str(tip_account).expect("Invalid pubkey");
    system_instruction::transfer(from_pubkey, &tip_pubkey, tip)
}

pub fn create_instruction_zeroslot(
    instructions: Vec<Instruction>,
    tip: u64,
) -> Vec<Instruction> {

    let keypair: &'static Keypair = get_wallet_keypair();

    let tip_ix = zeroslot_tip(
        tip,
        &keypair.pubkey(),
    );

    let mut result = vec![tip_ix];
    result.extend(instructions);
    result
}


pub async fn send_tx_zeroslot(tx: &Transaction) -> Result<String, Box<dyn std::error::Error>> {
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");

    // Pre-allocate buffer for serialization to avoid allocations
    let mut buffer = Vec::with_capacity(4096); // Pre-allocate 4KB buffer for larger transactions
    bincode::serialize_into(&mut buffer, tx)?;
    
    // Use a more efficient base64 encoding approach
    let base64_encoded_transaction = general_purpose::STANDARD.encode(&buffer);

    // Log transaction size for debugging
    let tx_size = base64_encoded_transaction.len();
    if tx_size > 10000 {
        eprintln!("[WARNING] Large transaction detected: {} bytes", tx_size);
    }

    // Build the JSON-RPC request (avoid cloning the URL)
    // Use a more efficient approach by pre-allocating the JSON structure
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [
            base64_encoded_transaction,
            {
                "encoding": "base64",
                "skipPreflight": true,
            }
        ]
    });

    // Send the request using the global client with connection pooling
    let response = HTTP_CLIENT
        .post(&config.zero_slot_url) // Use reference to avoid cloning
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    // Check response status
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("HTTP error {}: {}", status, error_text);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other, 
            format!("HTTP error {}: {}", status, error_text)
        )));
    } else {
        println!("Transaction sent successfully");
    }

    // Parse the response more efficiently
    let response_json: serde_json::Value = response.json().await?;
    
    if let Some(result) = response_json.get("result") {
        return Ok(result.to_string());
    } else if let Some(error) = response_json.get("error") {
        eprintln!("Failed to send transaction: {}", error);
        return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Failed to send transaction")));
    }

    // If neither result nor error is present, return a generic error
    Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Invalid response from sendTransaction")))
}