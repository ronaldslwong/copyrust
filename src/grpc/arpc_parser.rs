use crate::arpc::SubscribeResponse;

use crate::config_load::Config;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::utils::logger::{log_event, EventType};
use crate::utils::rt_scheduler::{set_realtime_priority, RealtimePriority};
use std::sync::Arc;
use core_affinity;

// use chrono::Local;


// Global atomic counter for ARPC messages
static ARPC_MESSAGE_COUNT: AtomicUsize = AtomicUsize::new(0);

// Function to get the current count (for testing/logging)
pub fn get_arpc_message_count() -> usize {
    ARPC_MESSAGE_COUNT.load(Ordering::Relaxed)
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
    
    let tx = resp.transaction.as_ref()?;
    let slot = tx.slot;
    let account_keys = Arc::new(tx.account_keys.clone());

    let detection_time = std::time::Instant::now();
    let tx_instructions = Arc::new(tx.instructions.clone());

    let sig_bytes = tx.signatures.get(0).map(|s| Arc::new(s.clone()));
    
    
    if let Some(ref sig_bytes) = sig_bytes {
        log_event(EventType::ArpcDetectionProcessing, sig_bytes, detection_time, None);
    }
    let parsed = ParsedArpcTrade {
        sig_bytes,
        slot,
        detection_time,
        tx_instructions,
        account_keys,
        // ... add more fields if needed ...
    };
    send_parsed_arpc_trade(parsed);

    // Optionally return None, or just return early
    None
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
