use crate::init::wallet_loader::{get_wallet_keypair, get_nonce_account};
use base64::{engine::general_purpose, Engine as _};
use once_cell::sync::Lazy;
use reqwest::Client;
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
use std::error::Error;
use chrono::Utc;

// Flashblock tip accounts
static FLASHBLOCK_TIP_ACCOUNTS: &[&str] = &[
    "FLaShB3iXXTWE1vu9wQsChUKq3HFtpMAhb8kAh1pf1wi",
    "FLashhsorBmM9dLpuq6qATawcpqk1Y2aqaZfkd48iT3W",
    "FLaSHJNm5dWYzEgnHJWWJP5ccu128Mu61NJLxUf7mUXU",
    "FLaSHR4Vv7sttd6TyDF4yR1bJyAxRwWKbohDytEMu3wL",
    "FLASHRzANfcAKDuQ3RXv9hbkBy4WVEKDzoAgxJ56DiE4",
    "FLasHstqx11M8W56zrSEqkCyhMCCpr6ze6Mjdvqope5s",
    "FLAShWTjcweNT4NSotpjpxAkwxUr2we3eXQGhpTVzRwy",
    "FLasHXTqrbNvpWFB6grN47HGZfK6pze9HLNTgbukfPSk",
    "FLAshyAyBcKb39KPxSzXcepiS8iDYUhDGwJcJDPX4g2B",
    "FLAsHZTRcf3Dy1APaz6j74ebdMC6Xx4g6i9YxjyrDybR",
];

// Global HTTP client with connection pooling for optimal performance
static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .pool_max_idle_per_host(50) // Keep up to 50 idle connections per host
        .pool_idle_timeout(std::time::Duration::from_secs(120)) // Keep connections alive for 2 minutes
        .tcp_keepalive(Some(std::time::Duration::from_secs(30))) // Enable TCP keep-alive
        .timeout(std::time::Duration::from_secs(3)) // 3 second timeout for larger transactions
        .connect_timeout(std::time::Duration::from_millis(500)) // 500ms connect timeout
        // Removed http2_prior_knowledge() to use HTTP/1.1 by default
        .build()
        .expect("Failed to create HTTP client")
});

/// Create a system transfer instruction for Flashblock tips
pub fn flashblock_tip(tip: u64, from_pubkey: &Pubkey) -> Instruction {
    // Randomly select a tip account from the list
    let tip_account = FLASHBLOCK_TIP_ACCOUNTS
        .choose(&mut rand::thread_rng())
        .expect("Failed to select random tip account");
    
    let tip_pubkey = Pubkey::from_str(tip_account).expect("Invalid pubkey");
    system_instruction::transfer(from_pubkey, &tip_pubkey, tip)
}

/// Build compute budget instructions for Flashblock with optimizations
pub fn create_instruction_flashblock(
    instructions: Vec<Instruction>,
    tip: u64,
    cu_price: u64,
    nonce_account: &Pubkey,
) -> Vec<Instruction> {
    let total_start = Instant::now();
    let now = Utc::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] 🔧 Starting Flashblock instruction building", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    
    // Step 1: Random number generation for compute unit price variation
    let rng_start = Instant::now();
    let mut rng = rand::thread_rng();
    let random_addition: u64 = rng.gen_range(1..=100);
    let adjusted_cu_price = cu_price + random_addition;
    let rng_time = rng_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] 🎲 RNG generation: {:.2?} (price: {} -> {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), rng_time, cu_price, adjusted_cu_price);
    
    // Step 2: Get wallet keypair
    let keypair_start = Instant::now();
    let keypair: &'static Keypair = get_wallet_keypair();
    let keypair_time = keypair_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] 🔑 Keypair access: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), keypair_time);
    
    // Step 3: Create compute budget instruction
    let compute_start = Instant::now();
    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(adjusted_cu_price);
    let compute_time = compute_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] 💰 Compute budget instruction: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), compute_time);
    
    // Step 4: Create tip instruction
    let tip_start = Instant::now();
    let tip_ix = flashblock_tip(tip, &keypair.pubkey());
    let tip_time = tip_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] 💸 Tip instruction creation: {:.2?} (tip: {} lamports)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), tip_time, tip);
    
    // Step 5: Create nonce instruction
    let nonce_start = Instant::now();
    let advance_nonce_ix = system_instruction::advance_nonce_account(
        nonce_account,
        &keypair.pubkey(),
    );
    let nonce_time = nonce_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] 🔄 Nonce instruction creation: {:.2?} (nonce: {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), nonce_time, nonce_account);
    
    // Step 6: Combine all instructions
    let combine_start = Instant::now();
    let mut result = vec![advance_nonce_ix, tip_ix, price_ix];
    result.extend(instructions);
    let combine_time = combine_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] 🔗 Instruction combination: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), combine_time);
    
    // Calculate total time
    let total_time = total_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_INSTRUCTION_PROFILE] ✅ Total instruction building time: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), total_time);
    
    result
}

/// Send a signed Solana transaction via Flashblock HTTP API with optimizations
pub async fn send_tx_flashblock(tx: &Transaction) -> Result<String, Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    let now = Utc::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_PROFILE] 🚀 Starting Flashblock send transaction", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    
    // Debug configuration
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] ⚙️  Flashblock URL: {}", config.flashblock_url);
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] 🔑 Flashblock API key length: {} chars", config.flashblock_api.len());
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] 💰 Flashblock CU price: {}", config.flashblock_cu_price);
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] 💸 Flashblock buy tip: {}", config.flashblock_buy_tip);
    
    // Step 1: Serialize transaction (measure serialization time)
    let serialize_start = Instant::now();
    let mut buffer = Vec::with_capacity(4096); // Pre-allocate 4KB buffer
    bincode::serialize_into(&mut buffer, tx)?;
    let serialize_time = serialize_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_PROFILE] 📦 Transaction serialization: {:.2?} (size: {} bytes)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), serialize_time, buffer.len());
    
    // Step 2: Base64 encoding (measure encoding time)
    let encode_start = Instant::now();
    let tx_b64 = general_purpose::STANDARD.encode(&buffer);
    let encode_time = encode_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_PROFILE] 🔤 Base64 encoding: {:.2?} (encoded size: {} chars)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), encode_time, tx_b64.len());
    
    // Step 3: Build request (measure request building time)
    let request_start = Instant::now();
    let request_body = serde_json::json!({
        "transactions": [tx_b64]
    });
    let request_time = request_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_PROFILE] 📝 Request building: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), request_time);
    
    // Step 4: Network call (measure network latency)
    let network_start = Instant::now();
    
    // Build the request URL and log it for debugging
    let request_url = format!("{}/api/v2/submit-batch", config.flashblock_url);
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] 🌐 Making POST request to: {}", request_url);
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] 🔑 Using API key: {}...", &config.flashblock_api[..config.flashblock_api.len().min(8)]);
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] 📦 Request body size: {} bytes", serde_json::to_string(&request_body).unwrap_or_default().len());
    #[cfg(feature = "verbose_logging")]
    println!("[FLASHBLOCK_DEBUG] 🔧 Using HTTP/1.1 (fallback from HTTP/2 due to server compatibility)");
    
    // Retry mechanism with exponential backoff
    let max_retries = 3;
    let mut attempt = 0;
    let mut response_result = None;
    
    while attempt < max_retries {
        attempt += 1;
        #[cfg(feature = "verbose_logging")]
        println!("[FLASHBLOCK_DEBUG] 🔄 Attempt {}/{}", attempt, max_retries);
        
        response_result = Some(HTTP_CLIENT
            .post(&request_url)
            .header("Content-Type", "application/json")
            .header("Authorization", &config.flashblock_api)
            .json(&request_body)
            .send()
            .await);
        
        match &response_result {
            Some(Ok(_)) => {
                #[cfg(feature = "verbose_logging")]
                println!("[FLASHBLOCK_DEBUG] ✅ Request successful on attempt {}", attempt);
                break;
            },
            Some(Err(e)) => {
                #[cfg(feature = "verbose_logging")]
                println!("[FLASHBLOCK_DEBUG] ❌ Attempt {} failed: {}", attempt, e);
                if attempt < max_retries {
                    let delay = std::time::Duration::from_millis(100 * attempt as u64);
                    #[cfg(feature = "verbose_logging")]
                    println!("[FLASHBLOCK_DEBUG] ⏳ Waiting {}ms before retry...", delay.as_millis());
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
    println!("[{}] - [FLASHBLOCK_PROFILE] 🌐 Network call: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), network_time);
    
    // Handle network errors with detailed logging
    let response = match response_result {
        Ok(resp) => {
            #[cfg(feature = "verbose_logging")]
            println!("[FLASHBLOCK_DEBUG] ✅ Request sent successfully");
            resp
        },
        Err(e) => {
            eprintln!("[FLASHBLOCK_DEBUG] ❌ Network request failed: {}", e);
            eprintln!("[FLASHBLOCK_DEBUG] 🔍 Error type: {:?}", e.status());
            eprintln!("[FLASHBLOCK_DEBUG] 🔍 Error source: {:?}", e.source());
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other, 
                format!("Network request failed: {}", e)
            )));
        }
    };
    
    // Step 5: Check response status with detailed logging
    let status = response.status();
    println!("[FLASHBLOCK_DEBUG] 📊 Response status: {} ({})", status, status.as_u16());
    println!("[FLASHBLOCK_DEBUG] 📋 Response headers: {:?}", response.headers());
    
    if !status.is_success() {
        let error_text = match response.text().await {
            Ok(text) => {
                println!("[FLASHBLOCK_DEBUG] ❌ Error response body: {}", text);
                text
            },
            Err(e) => {
                eprintln!("[FLASHBLOCK_DEBUG] ❌ Failed to read error response: {}", e);
                "Unknown error".to_string()
            }
        };
        
        eprintln!("[FLASHBLOCK] HTTP error {}: {}", status, error_text);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other, 
            format!("HTTP error {}: {}", status, error_text)
        )));
    }
    
    // Step 6: Parse response (measure response processing time)
    let response_start = Instant::now();
    let response_json: serde_json::Value = response.json().await?;
    let response_time = response_start.elapsed();
    
    // Debug: Print the actual response structure
    println!("[FLASHBLOCK_DEBUG] 📨 Raw response JSON: {}", serde_json::to_string_pretty(&response_json).unwrap_or_default());
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_PROFILE] 📨 Response processing: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), response_time);
    
    // Step 7: Extract signature from response
    let extract_start = Instant::now();
    let signature = if let Some(data) = response_json.get("data") {
        println!("[FLASHBLOCK_DEBUG] 📋 Found 'data' field in response");
        if let Some(transaction_ids) = data.get("transactionIds") {
            println!("[FLASHBLOCK_DEBUG] 📋 Found 'transactionIds' field in data");
            if let Some(ids) = transaction_ids.as_array() {
                println!("[FLASHBLOCK_DEBUG] 📋 transactionIds is an array with {} elements", ids.len());
                if let Some(first_id) = ids.first() {
                    let sig = first_id.as_str().unwrap_or("").to_string();
                    println!("[FLASHBLOCK_DEBUG] ✅ Extracted signature: {}", sig);
                    sig
                } else {
                    println!("[FLASHBLOCK_DEBUG] ❌ No first element in transactionIds array");
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other, 
                        "No transaction IDs in response"
                    )));
                }
            } else {
                println!("[FLASHBLOCK_DEBUG] ❌ transactionIds is not an array: {:?}", transaction_ids);
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other, 
                    "Invalid transaction IDs format"
                )));
            }
        } else {
            println!("[FLASHBLOCK_DEBUG] ❌ No 'transactionIds' field in data: {:?}", data);
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other, 
                "No transaction IDs in response data"
            )));
        }
    } else {
        println!("[FLASHBLOCK_DEBUG] ❌ No 'data' field in response: {:?}", response_json);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other, 
            "No data in response"
        )));
    };
    let extract_time = extract_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_PROFILE] 🔍 Signature extraction: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), extract_time);
    
    // Calculate total time and breakdown
    let total_time = total_start.elapsed();
    let processing_time = total_time - network_time; // Time spent in our code
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [FLASHBLOCK_PROFILE] ✅ Transaction sent successfully!", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    println!("[{}] - [FLASHBLOCK_PROFILE] 📊 Performance breakdown:", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    println!("[{}] - [FLASHBLOCK_PROFILE]   • Total time: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), total_time);
    println!("[{}] - [FLASHBLOCK_PROFILE]   • Network time: {:.2?} ({:.1}%)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), network_time, 
        (network_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
    println!("[{}] - [FLASHBLOCK_PROFILE]   • Processing time: {:.2?} ({:.1}%)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), processing_time,
        (processing_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
    println!("[{}] - [FLASHBLOCK_PROFILE]   • Signature: {}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), signature);
    
    Ok(signature)
}