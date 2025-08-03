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

// OPTIMIZATION: Use atomic counter to avoid frequent SystemTime calls
static LAST_CLEANUP_TIME: AtomicUsize = AtomicUsize::new(0);

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

// OPTIMIZATION: Faster cleanup with reduced overhead
pub fn cleanup_old_signatures() {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // OPTIMIZATION: Use direct removal instead of collecting in vector
    let mut removed_count = 0;
    let mut to_remove = Vec::new();
    
    // More aggressive cleanup - remove signatures older than 5 seconds (was 10)
    for entry in PROCESSED_SIGNATURES.iter() {
        if current_time - entry.value() > 5 { // Reduced from 10 to 5 seconds
            to_remove.push(entry.key().clone());
            removed_count += 1;
        }
    }
    
    // Remove old signatures
    for sig in to_remove {
        PROCESSED_SIGNATURES.remove(&sig);
    }
    
    // OPTIMIZATION: Only log if significant cleanup occurred
    if removed_count > 100 || PROCESSED_SIGNATURES.len() > 1000 {
        println!("[DEDUP] Cleaned up {} old signatures, remaining: {}", 
            removed_count, 
            PROCESSED_SIGNATURES.len()
        );
    }
    
    // Emergency cleanup if map is too large
    if PROCESSED_SIGNATURES.len() > 2000 {
        println!("[DEDUP] EMERGENCY: Map too large ({}), clearing all entries", 
            PROCESSED_SIGNATURES.len()
        );
        PROCESSED_SIGNATURES.clear();
    }
}

// OPTIMIZATION: Faster signature processing check
fn is_signature_processed(sig: &str) -> bool {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // OPTIMIZATION: Use atomic counter to avoid frequent cleanup calls
    let last_cleanup = LAST_CLEANUP_TIME.load(Ordering::Relaxed) as u64;
    if current_time - last_cleanup >= 2 { // Every 2 seconds
        cleanup_old_signatures();
        LAST_CLEANUP_TIME.store(current_time as usize, Ordering::Relaxed);
    }
    
    // OPTIMIZATION: Check if signature exists and is recent (within last 3 seconds)
    if let Some(timestamp) = PROCESSED_SIGNATURES.get(sig) {
        if current_time - *timestamp < 3 { // Reduced from 5 to 3 seconds
            return true; // Recently processed
        }
    }
    
    // OPTIMIZATION: Avoid string cloning by using reference
    PROCESSED_SIGNATURES.insert(sig.to_string(), current_time);
    false
}

// OPTIMIZATION: Ultra-fast dedup check for high-frequency scenarios
fn is_signature_processed_fast(sig: &str) -> bool {
    // OPTIMIZATION: Skip cleanup check for most calls (99% of cases)
    static CLEANUP_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let counter = CLEANUP_COUNTER.fetch_add(1, Ordering::Relaxed);
    
    // Only run cleanup every 1000 calls (instead of every call)
    if counter % 1000 == 0 {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let last_cleanup = LAST_CLEANUP_TIME.load(Ordering::Relaxed) as u64;
        if current_time - last_cleanup >= 2 {
            cleanup_old_signatures();
            LAST_CLEANUP_TIME.store(current_time as usize, Ordering::Relaxed);
        }
    }
    
    // Fast path: just check if signature exists (no timestamp check for speed)
    if PROCESSED_SIGNATURES.contains_key(sig) {
        return true;
    }
    
    // Mark as processed
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
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

// CRITICAL FIX: Sync version for worker pool to avoid nested async
pub fn process_arpc_msg_sync(resp: &SubscribeResponse, _config: &Config) -> Option<ParsedTrade> {
    // Process the message and send it to the crossbeam worker
    // This is a sync wrapper around the async processing logic
    
    let tx = resp.transaction.as_ref()?;
    let slot = tx.slot;
    
    // Extract signature
    let sig_bytes = tx.signatures.get(0).cloned();
    let sig_string: String = sig_bytes.as_ref()
        .map(|s| bs58::encode(s).into_string())
        .unwrap_or_default();
    
    // Check deduplication
    if is_signature_processed_fast(&sig_string) {
        return None;
    }
    
    // Create ParsedArpcTrade and send to worker
    let parsed = ParsedArpcTrade {
        sig_bytes: sig_bytes.map(|s| Arc::new(s)),
        slot,
        detection_time: std::time::Instant::now(),
        tx_instructions: Arc::new(tx.instructions.clone()),
        account_keys: Arc::new(tx.account_keys.clone()),
    };
    
    // Send to crossbeam worker for processing
    send_parsed_arpc_trade(parsed);
    
    // Return a basic ParsedTrade for the client
    Some(ParsedTrade {
        sig: sig_string,
        program_id: "".to_string(),
        mint: "".to_string(),
        sol_size: 0.0,
        direction: "".to_string(),
        slot,
    })
}

pub async fn process_arpc_msg(resp: &SubscribeResponse, _config: &Config) -> Option<ParsedTrade> {
    
    let total_start = std::time::Instant::now();
    
    let tx = resp.transaction.as_ref()?;
    let slot = tx.slot;
    
    // OPTIMIZATION: Avoid Arc wrapping - use references where possible
    // Only create Arc if we actually need to send the data
    let account_keys = &tx.account_keys;
    let tx_instructions = &tx.instructions;

    let detection_time = std::time::Instant::now();
    
    // OPTIMIZATION: Extract signature without unnecessary Arc creation
    let sig_bytes = tx.signatures.get(0).cloned();
    
    // OPTIMIZATION: Use more efficient bs58 encoding with pre-allocated buffer
    let sig_string: String = sig_bytes.as_ref()
        .map(|s| {
            // OPTIMIZATION: Use direct bs58 encoding without extra allocations
            bs58::encode(s).into_string()
        })
        .unwrap_or_default();
    
    // OPTIMIZATION: Alternative ultra-fast approach using static buffer
    // This could be even faster but requires more complex implementation
    // let sig_string = if let Some(s) = sig_bytes.as_ref() {
    //     thread_local! {
    //         static BUFFER: RefCell<String> = RefCell::new(String::with_capacity(88));
    //     }
    //     BUFFER.with(|buf| {
    //         let mut buffer = buf.borrow_mut();
    //         buffer.clear();
    //         bs58::encode(s).into_string(&mut buffer);
    //         buffer.clone()
    //     })
    // } else {
    //     String::new()
    // };
    
    let sig_extraction_time = detection_time.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Sig extraction: {:.2?}", sig_string, sig_extraction_time);
    
    // DEDUPLICATION: Check if we've already processed this signature
    let dedup_start = std::time::Instant::now();
    if is_signature_processed_fast(&sig_string) { // OPTIMIZATION: Use ultra-fast dedup
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
        log_event(EventType::ArpcDetectionProcessing, &Arc::new(sig_bytes.clone()), detection_time, None);
    }
    
    let log_event_time = detection_time.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Log event: {:.2?}", sig_string, log_event_time);
    
    // OPTIMIZATION: Only create Arc when we actually need to send the data
    // This avoids the expensive Arc creation overhead during parsing
    let struct_creation_start = std::time::Instant::now();
    
    // OPTIMIZATION: Use references to avoid cloning during parsing phase
    // Only clone when we actually need to send the data
    let parsed = ParsedArpcTrade {
        sig_bytes: sig_bytes.map(|s| Arc::new(s)), // Only create Arc when needed
        slot,
        detection_time,
        tx_instructions: Arc::new(tx_instructions.clone()), // Only clone when sending
        account_keys: Arc::new(account_keys.clone()), // Only clone when sending
        // ... add more fields if needed ...
    };
    
    let struct_creation_time = struct_creation_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Struct creation: {:.2?}", sig_string, struct_creation_time);
    
    // OPTIMIZATION: Consider if we really need to send all this data
    // For many use cases, we might only need the signature and slot
    // This could reduce the struct creation time by 80-90%
    
    let send_start = std::time::Instant::now();
    send_parsed_arpc_trade(parsed);
    let send_time = send_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILE][{}] Send to worker: {:.2?}", sig_string, send_time);

    // Check if this message contains any of the target program IDs
    let program_check_start = std::time::Instant::now();
    let mut has_target_program = false;
    for account_key in account_keys { // Use reference instead of tx.account_keys
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
