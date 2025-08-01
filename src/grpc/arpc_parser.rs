use crate::arpc::SubscribeResponse;

use crate::config_load::Config;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::utils::logger::{log_event, EventType};
use std::sync::Arc;
use crate::constants::raydium_launchpad::RAYDIUM_LAUNCHPAD_PROGRAM_ID_BYTES;
use crate::constants::axiom::{AXIOM_PUMP_SWAP_PROGRAM_ID_BYTES, AXIOM_PUMP_FUN_PROGRAM_ID_BYTES};
use crate::constants::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID_BYTES;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::time::{SystemTime, UNIX_EPOCH};

// use chrono::Local;

// Global deduplication set - tracks processed signatures with timestamps
static PROCESSED_SIGNATURES: Lazy<DashMap<String, u64>> = Lazy::new(|| {
    DashMap::new()
});

// Global atomic counter for ARPC messages
static ARPC_MESSAGE_COUNT: AtomicUsize = AtomicUsize::new(0);

// Function to get the current count (for testing/logging)
pub fn get_arpc_message_count() -> usize {
    ARPC_MESSAGE_COUNT.load(Ordering::Relaxed)
}

// Function to get deduplication stats
pub fn get_dedup_stats() -> usize {
    PROCESSED_SIGNATURES.len()
}

// Function to clean up old signatures (older than 5 seconds)
pub fn cleanup_old_signatures() {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut to_remove = Vec::new();
    
    // More aggressive cleanup - remove signatures older than 5 seconds (was 10)
    for entry in PROCESSED_SIGNATURES.iter() {
        if current_time - entry.value() > 5 { // Reduced from 10 to 5 seconds
            to_remove.push(entry.key().clone());
        }
    }
    
    // Remove old signatures
    let removed_count = to_remove.len();
    for sig in to_remove {
        PROCESSED_SIGNATURES.remove(&sig);
    }
    
    // Log cleanup if we removed many entries or if map is getting large
    if removed_count > 50 || PROCESSED_SIGNATURES.len() > 1000 {
        let now = chrono::Utc::now();
        println!("[{}] - [DEDUP] Cleaned up {} old signatures, remaining: {}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), 
            removed_count, 
            PROCESSED_SIGNATURES.len()
        );
    }
    
    // Emergency cleanup if map is too large
    if PROCESSED_SIGNATURES.len() > 2000 {
        let now = chrono::Utc::now();
        println!("[{}] - [DEDUP] EMERGENCY: Map too large ({}), clearing all entries", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), 
            PROCESSED_SIGNATURES.len()
        );
        PROCESSED_SIGNATURES.clear();
    }
}

// Function to check if signature was already processed
fn is_signature_processed(sig: &str) -> bool {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Clean up old signatures more frequently (every 2 seconds instead of 5)
    if current_time % 2 == 0 { // Every 2 seconds (was 5)
        cleanup_old_signatures();
    }
    
    // Check if signature exists and is recent (within last 3 seconds instead of 5)
    if let Some(timestamp) = PROCESSED_SIGNATURES.get(sig) {
        if current_time - *timestamp < 3 { // Reduced from 5 to 3 seconds
            return true; // Recently processed
        }
    }
    
    // Mark as processed
    PROCESSED_SIGNATURES.insert(sig.to_string(), current_time);
    false
}

// Call this in your ARPC message handler:
// ARPC_MESSAGE_COUNT.fetch_add(1, Ordering::Relaxed);

#[derive(Debug, Clone)]
pub struct ParsedTrade {
    pub sig: String,
    pub program_id: String,
    pub mint: String,
    pub sol_size: f64,
    pub direction: String,
    pub slot: u64,
}


pub use crate::grpc::arpc_worker::{ParsedArpcTrade, setup_arpc_crossbeam_worker, send_parsed_arpc_trade};

pub async fn process_arpc_msg(resp: &SubscribeResponse, _config: &Config) -> Option<ParsedTrade> {
    
    let total_start = std::time::Instant::now();
    
    let tx = resp.transaction.as_ref()?;
    let slot = tx.slot;
    let account_keys = Arc::new(tx.account_keys.clone());

    let detection_time = std::time::Instant::now();
    let tx_instructions = Arc::new(tx.instructions.clone());

    // Most aggressive optimization - extract signature directly without Arc wrapping
    let sig_bytes = tx.signatures.get(0).map(|s| Arc::new(s.clone()));
    
    // Extract signature string efficiently
    let sig_string: String = sig_bytes.as_ref().map(|s| bs58::encode(s.as_slice()).into_string()).unwrap_or_default();
    
    let sig_extraction_time = detection_time.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Sig extraction: {:.2?}", sig_string, sig_extraction_time);
    
    // DEDUPLICATION: Check if we've already processed this signature
    let dedup_start = std::time::Instant::now();
    if is_signature_processed(&sig_string) {
        // Skip processing - already handled
        let dedup_time = dedup_start.elapsed();
        #[cfg(feature = "verbose_logging")]
        println!("[PROFILE][{}] Dedup check (skipped): {:.2?}", sig_string, dedup_time);
        return None;
    }
    let dedup_time = dedup_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Dedup check (passed): {:.2?}", sig_string, dedup_time);
    
    if let Some(ref sig_bytes) = sig_bytes {
        log_event(EventType::ArpcDetectionProcessing, sig_bytes, detection_time, None);
    }
    
    let log_event_time = detection_time.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Log event: {:.2?}", sig_string, log_event_time);
    
    let parsed = ParsedArpcTrade {
        sig_bytes,
        slot,
        detection_time,
        tx_instructions,
        account_keys,
        // ... add more fields if needed ...
    };
    
    let struct_creation_time = detection_time.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Struct creation: {:.2?}", sig_string, struct_creation_time);
    
    let send_start = std::time::Instant::now();
    send_parsed_arpc_trade(parsed);
    let send_time = send_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Send to worker: {:.2?}", sig_string, send_time);

    // Check if this message contains any of the target program IDs
    let program_check_start = std::time::Instant::now();
    let mut has_target_program = false;
    for account_key in &tx.account_keys {
        if account_key == &*crate::constants::raydium_launchpad::RAYDIUM_LAUNCHPAD_PROGRAM_ID_BYTES ||
           account_key == &*crate::constants::axiom::AXIOM_PUMP_SWAP_PROGRAM_ID_BYTES ||
           account_key == &*crate::constants::axiom::AXIOM_PUMP_FUN_PROGRAM_ID_BYTES ||
           account_key == &*crate::constants::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID_BYTES {
            has_target_program = true;
            break;
        }
    }
    let program_check_time = program_check_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Program ID check: {:.2?}", sig_string, program_check_time);

    let total_time = total_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] TOTAL parser time: {:.2?}", sig_string, total_time);

    // Return a dummy ParsedTrade if we have target programs (this will increment the processed counter)
    if has_target_program {
        Some(ParsedTrade {
            sig: sig_string,
            program_id: "target_program".to_string(),
            mint: "unknown".to_string(),
            sol_size: 0.0,
            direction: "unknown".to_string(),
            slot,
        })
    } else {
        None
    }
}

/// Parses a slice of bytes using a sliding window, printing any valid 32-byte chunk
/// that can be interpreted as a Solana public key. This is useful for finding keys
/// that may not be aligned on a 32-byte boundary.
///
/// # Arguments
///
/// * `data` - A slice of bytes to be scanned.
pub fn print_pubkeys_from_data(data: &[u8]) {
    println!("--- Scanning Data for Pubkeys (Sliding Window) ---");
    if data.len() < 32 {
        println!("  Data is too short to contain a valid Pubkey.");
        println!("--------------------------------------------------");
        return;
    }

    for i in 0..=(data.len() - 32) {
        let chunk = &data[i..i + 32];
        if let Ok(pubkey) = Pubkey::try_from(chunk) {
            // To avoid spamming with system program or other common keys,
            // you might add a filter here if needed.
            println!("  Found Pubkey at index {}: {}", i, pubkey.to_string());
        }
    }
    println!("--------------------------------------------------");
}
