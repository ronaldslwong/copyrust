use crate::init::wallet_loader::{get_wallet_keypair, get_nonce_account};
use crate::init::tip_stream::get_tip_percentile;
use base64::{engine::general_purpose, Engine as _};
use isahc::{HttpClient, prelude::*};
use rand::seq::SliceRandom;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use solana_sdk::instruction::Instruction;
use crate::config_load::GLOBAL_CONFIG;
use rand::Rng;
use solana_sdk::compute_budget;
use std::str::FromStr;
use std::time::Instant;

use chrono::Utc;
use std::sync::OnceLock;

// Astralane tip accounts as specified in the documentation
static ASTRALANE_TIP_ACCOUNTS: &[&str] = &[
    "astrazznxsGUhWShqgNtAdfrzP2G83DzcWVJDxwV9bF",
    "astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm",
    "astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk",
    "astraRVUuTHjpwEVvNBeQEgwYx9w9CFyfxjYoobCZhL",
];

// Optimized isahc client with TCP_NODELAY and persistent connections
static ISAHC_CLIENT: OnceLock<HttpClient> = OnceLock::new();

fn get_isahc_client() -> HttpClient {
    ISAHC_CLIENT.get_or_init(|| {
        HttpClient::builder()
            .max_connections_per_host(50) // Allow up to 50 connections per host
            .timeout(std::time::Duration::from_secs(3)) // 3 second timeout
            .connect_timeout(std::time::Duration::from_millis(500)) // 500ms connect timeout
            .build()
            .expect("Failed to create isahc client")
    }).clone()
}

/// Create a system transfer instruction for Astralane tips
pub fn astralane_tip(tip: u64, from_pubkey: &Pubkey) -> Instruction {
    // Randomly select a tip account from the list
    let tip_account = ASTRALANE_TIP_ACCOUNTS
        .choose(&mut rand::thread_rng())
        .expect("Failed to select random tip account");
    
    let tip_pubkey = Pubkey::from_str(tip_account).expect("Invalid pubkey");
    system_instruction::transfer(from_pubkey, &tip_pubkey, tip)
}

/// Build compute budget instructions for Astralane with optimizations
pub fn create_instruction_astralane(
    instructions: Vec<Instruction>,
    tip: u64,
    cu_price: u64,
    nonce_account: &Pubkey,
) -> Vec<Instruction> {
    let total_start = Instant::now();
    let now = Utc::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] 🔧 Starting Astralane instruction building", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    
    // Step 1: Random number generation for compute unit price variation
    let rng_start = Instant::now();
    let mut rng = rand::thread_rng();
    let random_addition: u64 = rng.gen_range(1..=100);
    let adjusted_cu_price = cu_price + random_addition;
    let rng_time = rng_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] 🎲 RNG generation: {:.2?} (price: {} -> {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), rng_time, cu_price, adjusted_cu_price);
    
    // Step 2: Get wallet keypair
    let keypair_start = Instant::now();
    let keypair: &'static Keypair = get_wallet_keypair();
    let keypair_time = keypair_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] 🔑 Keypair access: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), keypair_time);
    
    // Step 3: Create compute budget instruction
    let compute_start = Instant::now();
    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(adjusted_cu_price);
    let compute_time = compute_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] 💰 Compute budget instruction: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), compute_time);
    
    // Step 4: Create tip instruction
    let tip_start = Instant::now();
    let tip_ix = astralane_tip(tip, &keypair.pubkey());
    let tip_time = tip_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] 💸 Tip instruction creation: {:.2?} (tip: {} lamports)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), tip_time, tip);
    
    // Step 5: Create nonce instruction
    let nonce_start = Instant::now();
    let advance_nonce_ix = system_instruction::advance_nonce_account(
        nonce_account,
        &keypair.pubkey(),
    );
    let nonce_time = nonce_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] 🔄 Nonce instruction creation: {:.2?} (nonce: {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), nonce_time, nonce_account);
    
    // Step 6: Combine all instructions
    let combine_start = Instant::now();
    let mut result = vec![advance_nonce_ix, tip_ix, price_ix];
    result.extend(instructions);
    let combine_time = combine_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] 🔗 Instruction combination: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), combine_time);
    
    // Calculate total time
    let total_time = total_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_INSTRUCTION_PROFILE] ✅ Total instruction building time: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), total_time);
    
    result
}

/// Send a signed Solana transaction via Astralane HTTP API with isahc optimizations
/// This implements the sendTransaction method as described in the Astralane documentation
pub async fn send_tx_astralane(tx: &Transaction) -> Result<String, Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    let now = Utc::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] 🚀 Starting Astralane send transaction (ISAHC)", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    
    // Debug configuration
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] ⚙️  Astralane URL: {}", config.astralane_url);
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] 💰 Astralane CU price: {}", config.astralane_cu_price);
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] 💸 Astralane buy tip: {}", config.astralane_buy_tip);
    
    // Step 1: Serialize transaction (measure serialization time)
    let serialize_start = Instant::now();
    let mut buffer = Vec::with_capacity(4096); // Pre-allocate 4KB buffer
    bincode::serialize_into(&mut buffer, tx)?;
    let serialize_time = serialize_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] 📦 Transaction serialization: {:.2?} (size: {} bytes)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), serialize_time, buffer.len());
    
    // Step 2: Base64 encoding (measure encoding time)
    let encode_start = Instant::now();
    let tx_b64 = general_purpose::STANDARD.encode(&buffer);
    let encode_time = encode_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] 🔤 Base64 encoding: {:.2?} (encoded size: {} chars)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), encode_time, tx_b64.len());
    
    // Step 3: Pre-allocate JSON payload string for better performance
    let request_start = Instant::now();
    let request_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":["{}",{{"skipPreflight":true,"encoding":"base64"}}]}}"#,
        tx_b64
    );
    let request_time = request_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] 📝 Request building: {:.2?} (size: {} bytes)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), request_time, request_json.len());
    
    // Step 4: Network call with isahc (measure network latency)
    let network_start = Instant::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] 🌐 Making POST request to: {}", config.astralane_url);
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] 📦 Request body size: {} bytes", request_json.len());
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] 🔧 Using ISAHC with optimized connection pooling");
    
    // Retry mechanism with exponential backoff
    let max_retries = 3;
    let mut attempt = 0;
    let mut response_result = None;
    
    while attempt < max_retries {
        attempt += 1;
        #[cfg(feature = "verbose_logging")]
        println!("[ASTRALANE_DEBUG] 🔄 Attempt {}/{}", attempt, max_retries);
        
        // Use our persistent client to keep connections warm
        response_result = Some(get_isahc_client()
            .post_async(&config.astralane_url, request_json.clone())
            .await);
        
        match &response_result {
            Some(Ok(_)) => {
                #[cfg(feature = "verbose_logging")]
                println!("[ASTRALANE_DEBUG] ✅ Request successful on attempt {}", attempt);
                break;
            },
            Some(Err(e)) => {
                #[cfg(feature = "verbose_logging")]
                println!("[ASTRALANE_DEBUG] ❌ Attempt {} failed: {}", attempt, e);
                if attempt < max_retries {
                    let delay = std::time::Duration::from_millis(100 * attempt as u64);
                    #[cfg(feature = "verbose_logging")]
                    println!("[ASTRALANE_DEBUG] ⏳ Waiting {}ms before retry...", delay.as_millis());
                    tokio::time::sleep(delay).await;
                }
            },
            None => break,
        }
    }
    
    let response_result = match response_result {
        Some(result) => result,
        None => {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other, 
                "All retry attempts failed"
            )) as Box<dyn std::error::Error + Send + Sync>);
        }
    };
    
    let network_time = network_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] 🌐 Network call: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), network_time);
    
    // Handle network errors with detailed logging
    let mut response = match response_result {
        Ok(resp) => {
            #[cfg(feature = "verbose_logging")]
            println!("[ASTRALANE_DEBUG] ✅ Request sent successfully");
            resp
        },
        Err(e) => {
            eprintln!("[ASTRALANE_DEBUG] ❌ Network request failed: {}", e);
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other, 
                format!("Network request failed: {}", e)
            )));
        }
    };
    
    // Step 5: Check response status
    let status = response.status();
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] 📊 Response status: {} ({})", status, status.as_u16());
    
    if !status.is_success() {
        let error_text = match response.text().await {
            Ok(text) => {
                #[cfg(feature = "verbose_logging")]
                println!("[ASTRALANE_DEBUG] ❌ Error response body: {}", text);
                text.to_string()
            },
            Err(e) => {
                #[cfg(feature = "verbose_logging")]
                eprintln!("[ASTRALANE_DEBUG] ❌ Failed to read error response: {}", e);
                "Unknown error".to_string()
            }
        };
        
        eprintln!("[ASTRALANE] HTTP error {}: {}", status, error_text);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other, 
            format!("HTTP error {}: {}", status, error_text)
        )));
    }
    
    // Step 6: Manual JSON extraction for better performance
    let response_start = Instant::now();
    let response_text = response.text().await?.to_string();
    let response_time = response_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] 📨 Response processing: {:.2?} (size: {} bytes)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), response_time, response_text.len());
    
    // Debug: Print the actual response structure
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] 📨 Raw response: {}", response_text);
    
    // Step 7: Manual JSON parsing for signature extraction (avoid serde_json overhead)
    let extract_start = Instant::now();
    let signature = extract_signature_manual(&response_text)?;
    let extract_time = extract_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] 🔍 Signature extraction: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), extract_time);
    
    // Calculate total time and breakdown
    let total_time = total_start.elapsed();
    let processing_time = total_time - network_time; // Time spent in our code
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [ASTRALANE_PROFILE] ✅ Transaction sent successfully!", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    println!("[{}] - [ASTRALANE_PROFILE] 📊 Performance breakdown:", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    println!("[{}] - [ASTRALANE_PROFILE]   • Total time: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), total_time);
    println!("[{}] - [ASTRALANE_PROFILE]   • Network time: {:.2?} ({:.1}%)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), network_time, 
        (network_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
    println!("[{}] - [ASTRALANE_PROFILE]   • Processing time: {:.2?} ({:.1}%)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), processing_time,
        (processing_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
    println!("[{}] - [ASTRALANE_PROFILE]   • Signature: {}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), signature);
    
    Ok(signature)
}

/// Manual signature extraction for better performance (avoid serde_json overhead)
fn extract_signature_manual(response: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Look for "result" field in the JSON response
    if let Some(result_start) = response.find(r#""result":"#) {
        let signature_start = result_start + 10; // Length of "result":
        if let Some(signature_end) = response[signature_start..].find('"') {
            let signature = &response[signature_start..signature_start + signature_end];
            #[cfg(feature = "verbose_logging")]
            println!("[ASTRALANE_DEBUG] ✅ Manually extracted signature: {}", signature);
            return Ok(signature.to_string());
        }
    }
    
    // Check for error field
    if response.contains(r#""error":"#) {
        #[cfg(feature = "verbose_logging")]
        println!("[ASTRALANE_DEBUG] ❌ Error in response: {}", response);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other, 
            format!("Astralane error in response: {}", response)
        )));
    }
    
    #[cfg(feature = "verbose_logging")]
    println!("[ASTRALANE_DEBUG] ❌ No signature found in response: {}", response);
    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other, 
        "No signature found in response"
    )))
}

