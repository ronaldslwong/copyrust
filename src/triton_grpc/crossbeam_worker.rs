use crossbeam::channel::{bounded, Sender};
use once_cell::sync::OnceCell;
use bs58;
use core_affinity;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::Instruction;
use chrono::Utc;
use solana_transaction_status;
use crate::utils::logger::{log_event, setup_event_logger, EventType};
use crate::utils::rt_scheduler::{set_realtime_priority, RealtimePriority};

// use tokio::time::{sleep, Duration};
use crate::grpc::arpc_worker::GLOBAL_TX_MAP;
use crate::build_tx::pump_fun::{build_sell_instruction, get_bonding_curve_state, BondingCurve};
use crate::init::wallet_loader::get_wallet_keypair;
use crate::build_tx::pump_swap::build_pump_sell_instruction;

use crate::build_tx::ray_launch::build_ray_launch_sell_instruction;
use crate::build_tx::ray_cpmm::{build_ray_cpmm_sell_instruction};
use crate::send_tx::rpc::send_tx_via_send_rpcs;
use crate::send_tx::zero_slot::{create_instruction_zeroslot, send_tx_zeroslot};
use crate::build_tx::tx_builder::{create_instruction};
use crate::build_tx::tx_builder::build_and_sign_transaction_fast;
use solana_sdk::signature::Signer;
use crate::config_load::GLOBAL_CONFIG;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use borsh::BorshDeserialize;
use std::time::Instant;
use std::thread;
use std::time::Duration;
use crate::grpc::monitoring_client::GLOBAL_MONITORING_DATA;
use crate::send_tx::jito::send_jito_bundle;
use crate::send_tx::jito::create_instruction_jito;
use crate::send_tx::generic_sender::send_all_vendors_parallel;
use crate::grpc::utils;


// Add global counters for monitoring triton worker performance
use std::sync::atomic::{AtomicUsize, Ordering};
static TRITON_MESSAGES_RECEIVED: AtomicUsize = AtomicUsize::new(0);
static TRITON_TRANSACTIONS_SENT: AtomicUsize = AtomicUsize::new(0);
static TRITON_TRANSACTIONS_FOUND: AtomicUsize = AtomicUsize::new(0);
static TRITON_ERRORS: AtomicUsize = AtomicUsize::new(0);

// OPTIMIZATION: Add performance monitoring
static TRITON_PROCESSING_TIMES: AtomicUsize = AtomicUsize::new(0);
static TRITON_TOTAL_PROCESSING_TIME: AtomicUsize = AtomicUsize::new(0);

pub fn get_triton_stats() -> (usize, usize, usize, usize) {
    (
        TRITON_MESSAGES_RECEIVED.load(Ordering::Relaxed),
        TRITON_TRANSACTIONS_SENT.load(Ordering::Relaxed),
        TRITON_TRANSACTIONS_FOUND.load(Ordering::Relaxed),
        TRITON_ERRORS.load(Ordering::Relaxed),
    )
}

// OPTIMIZATION: Get average processing time
pub fn get_triton_avg_processing_time() -> f64 {
    let total_time = TRITON_TOTAL_PROCESSING_TIME.load(Ordering::Relaxed);
    let count = TRITON_PROCESSING_TIMES.load(Ordering::Relaxed);
    if count > 0 {
        total_time as f64 / count as f64
    } else {
        0.0
    }
}

// OPTIMIZATION: Get detailed performance stats
pub fn get_triton_performance_stats() -> (usize, usize, usize, usize, f64, f64) {
    let (received, sent, found, errors) = get_triton_stats();
    let avg_time = get_triton_avg_processing_time();
    let total_time = TRITON_TOTAL_PROCESSING_TIME.load(Ordering::Relaxed) as f64;
    
    (received, sent, found, errors, avg_time, total_time)
}

// OPTIMIZATION: Print performance summary
pub fn print_triton_performance_summary() {
    let (received, sent, found, errors, avg_time, total_time) = get_triton_performance_stats();
    
    println!("=== TRITON PERFORMANCE SUMMARY ===");
    println!("Messages Received: {}", received);
    println!("Transactions Sent: {}", sent);
    println!("Transactions Found: {}", found);
    println!("Errors: {}", errors);
    println!("Average Processing Time: {:.2}µs", avg_time);
    println!("Total Processing Time: {:.2}µs", total_time);
    if received > 0 {
        println!("Success Rate: {:.2}%", (sent as f64 / received as f64) * 100.0);
        println!("Find Rate: {:.2}%", (found as f64 / received as f64) * 100.0);
    }
    println!("================================");
}

// Create a global Tokio runtime for async operations
use once_cell::sync::Lazy;
static ASYNC_RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Runtime::new().expect("Failed to create async runtime")
});

#[derive(Debug, Clone)]
pub struct ParsedTx {
    pub sig_bytes: Option<Vec<u8>>,
    pub is_signer: bool,
    pub slot: Option<u64>,
    pub detection_time: Option<Instant>,
    pub feed_id: String, // OPTIMIZATION: Add feed identification
    pub pre_token_balances: Option<Vec<solana_transaction_status::UiTransactionTokenBalance>>,
    pub post_token_balances: Option<Vec<solana_transaction_status::UiTransactionTokenBalance>>,
    // Add more fields as needed
}

// Note: ParsedTxWithTokenBalances removed to avoid type conflicts
// We'll use the existing RPC-based approach for now

// OPTIMIZATION: Global deduplication for multiple feeds
use std::collections::HashMap;
use dashmap::DashMap;

// Track which feed first detected each signature (lock-free)
static FEED_DEDUP_MAP: Lazy<DashMap<String, (String, Instant)>> = Lazy::new(|| {
    DashMap::new()
});

// OPTIMIZATION: Fast feed deduplication check (lock-free)
pub fn is_signature_processed_by_feed(sig: &str, feed_id: &str) -> bool {
    // Check if already processed (lock-free read)
    if FEED_DEDUP_MAP.contains_key(sig) {
        return true;
    }
    
    // Try to insert (atomic operation)
    let entry = FEED_DEDUP_MAP.entry(sig.to_string());
    match entry {
        dashmap::mapref::entry::Entry::Occupied(_) => {
            // Another thread beat us to it
            true
        }
        dashmap::mapref::entry::Entry::Vacant(vacant) => {
            // We're the first to process this signature
            vacant.insert((feed_id.to_string(), Instant::now()));
            false
        }
    }
}

// OPTIMIZATION: Cleanup old deduplication entries to prevent memory leaks
pub fn cleanup_feed_dedup_map() {
    let current_time = Instant::now();
    let mut to_remove = Vec::new();
    
    // Remove entries older than 30 seconds
    for entry in FEED_DEDUP_MAP.iter() {
        if current_time.duration_since(entry.value().1) > Duration::from_secs(30) {
            to_remove.push(entry.key().clone());
        }
    }
    
    // Remove old entries
    for key in to_remove {
        FEED_DEDUP_MAP.remove(&key);
    }
    
    // Emergency cleanup if map gets too large
    if FEED_DEDUP_MAP.len() > 5000 {
        println!("[Triton] WARNING: Feed dedup map too large ({} entries), clearing...", FEED_DEDUP_MAP.len());
        FEED_DEDUP_MAP.clear();
    }
}

static PARSED_TX_SENDER: OnceCell<Sender<ParsedTx>> = OnceCell::new();

/// Call this once at startup (e.g., in main.rs) to spawn the worker thread.
pub fn setup_crossbeam_worker() {
    // OPTIMIZATION: Use bounded channel instead of unbounded to prevent memory leaks
    let (tx, rx) = bounded::<ParsedTx>(1000);  // Changed from unbounded to bounded with 1000 capacity
    PARSED_TX_SENDER.set(tx).unwrap();
    
    // Start deduplication cleanup task
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            cleanup_feed_dedup_map();
        }
    });
    
    // Spawn 3 worker threads for heavy processing
    for worker_id in 0..3 {
        let rx_clone = rx.clone();
        std::thread::spawn(move || {
            // Pin worker threads to cores 2-4 for optimal performance
            if let Some(cores) = core_affinity::get_core_ids() {
                if cores.len() > 2 + worker_id {
                    core_affinity::set_for_current(cores[2 + worker_id]);
                    println!("[triton crossbeam worker {}] Pinned to core {}", worker_id, 2 + worker_id);
                }
            }
            
            // Set critical real-time priority for processing (highest priority)
            if let Err(e) = set_realtime_priority(RealtimePriority::Critical) {
                eprintln!("[triton crossbeam worker {}] Failed to set real-time priority: {}", worker_id, e);
            }
            
            let mut consecutive_errors = 0;
            const MAX_CONSECUTIVE_ERRORS: usize = 10;
            
            while let Ok(parsed) = rx_clone.recv() {
                let receive_start = Instant::now();
                let processing_start = Instant::now();
                TRITON_MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);
                

                
                // OPTIMIZATION: Fast signature extraction
                let sig_extract_start = Instant::now();
                let sig_detect = if let Some(sig) = &parsed.sig_bytes {
                    bs58::encode(sig).into_string()
                } else {
                    String::new()
                };
                let sig_extract_time = sig_extract_start.elapsed();
                // Initialize profiling variables
                let mut map_search_time = std::time::Duration::ZERO;
                let mut wait_time = std::time::Duration::ZERO;
                let mut build_time = std::time::Duration::ZERO;
                let mut send_time = std::time::Duration::ZERO;
                let mut buy_send_time = std::time::Duration::ZERO;
                let mut rpc_time = std::time::Duration::ZERO;
                let mut is_signer_check_time = std::time::Duration::ZERO;
                let mut map_size_time = std::time::Duration::ZERO;
                let mut found_check_time = std::time::Duration::ZERO;
                let mut sig_bytes_check_time = std::time::Duration::ZERO;
                let mut map_get_time = std::time::Duration::ZERO;

                // OPTIMIZATION: Only log in verbose mode
                {
                    let now = Utc::now();
                    println!("[{}] - [TRITON-{}] Processing message for sig: {} (feed: {}) (total received: {}) (sig_extract: {:.2?})", 
                        now.format("%Y-%m-%d %H:%M:%S%.3f"), 
                        worker_id,
                        sig_detect, 
                        parsed.feed_id,
                        TRITON_MESSAGES_RECEIVED.load(Ordering::Relaxed),
                        sig_extract_time);
                }

                if parsed.detection_time.is_none() {
                    consecutive_errors += 1;
                    TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                    eprintln!("[crossbeam_worker] Error: detection_time is None for sig_detect={}", sig_detect);
                    
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        eprintln!("[Triton worker {}] Too many consecutive errors ({}), restarting...", worker_id, consecutive_errors);
                        break; // Exit to trigger restart
                    }
                    continue;
                }
                
                consecutive_errors = 0; // Reset error counter on successful processing

                let config = match GLOBAL_CONFIG.get() {
                    Some(cfg) => cfg,
                    None => {
                        eprintln!("[crossbeam_worker] Error: Config not initialized");
                        continue;
                    }
                };

                let mut found = None;
                
                // OPTIMIZATION: Only log in verbose mode
                #[cfg(feature = "verbose_logging")]
                {
                    let now = Utc::now();
                    println!("[{}] - [TRITON-{}] Processing message for sig: {} (feed: {}) (total received: {})", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_detect, parsed.feed_id, TRITON_MESSAGES_RECEIVED.load(Ordering::Relaxed));
                }
                
                let is_signer_check_start = Instant::now();
                let is_signer = parsed.is_signer;
                let is_signer_check_time = is_signer_check_start.elapsed();
                
                if is_signer {
                    let map_size_start = Instant::now();
                    let map_size = GLOBAL_TX_MAP.len();
                    let map_size_time = map_size_start.elapsed();
                    
                    // OPTIMIZATION: Only log in verbose mode
                    {
                        let now = Utc::now();
                        println!("[{}] - [TRITON-{}] Searching GLOBAL_TX_MAP for sig: {} (feed: {}) (map size: {})", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_detect, parsed.feed_id, map_size);
                    }
                    
                    // OPTIMIZATION: Fast map search
                    let map_search_start = Instant::now();
                    for entry in GLOBAL_TX_MAP.iter() {
                        if entry.value().send_sig.trim_matches('\"') == sig_detect {
                            found = Some(entry.value().clone());
                            TRITON_TRANSACTIONS_FOUND.fetch_add(1, Ordering::Relaxed);
                            
                            // OPTIMIZATION: Only log in verbose mode
                            {
                                let now = Utc::now();
                                println!("[{}] - [TRITON-{}] FOUND transaction in map for sig: {} (feed: {}) (tx_type: {})", 
                                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_detect, parsed.feed_id, entry.value().tx_type);
                            }
                            break;
                        }
                    }
                    map_search_time = map_search_start.elapsed();
                    
                    // OPTIMIZATION: Only log in verbose mode
                    if found.is_none() {
                        let now = Utc::now();
                        println!("[{}] - [TRITON-{}] NOT FOUND transaction in map for sig: {} (feed: {}) (map size: {})", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_detect, parsed.feed_id, map_size);
                    }
                    
                    let found_check_start = Instant::now();
                    let found_check = found.is_some();
                    let found_check_time = found_check_start.elapsed();
                    
                    if let Some(mut tx_with_pubkey) = found {
                        let now = Utc::now();
                        let mut send_tx: bool = false;

                        let sig_bytes = parsed.sig_bytes.as_ref().unwrap();
                        
                        // OPTIMIZATION: Only log in verbose mode

                        log_event(
                            EventType::GrpcLanded,
                            sig_bytes,
                            tx_with_pubkey.send_time,
                            Some((parsed.slot.unwrap() - tx_with_pubkey.send_slot) as i64)
                        );

                        // Use configurable wait time instead of hardcoded 4 seconds
                        let wait_time_secs = config.wait_time as u64;
                        let wait_start = Instant::now();
                        thread::sleep(Duration::from_secs(wait_time_secs));
                        wait_time = wait_start.elapsed();
                        let mut sell_instruction: Instruction = Instruction{
                            program_id: Pubkey::new_unique(),
                            accounts: vec![],
                            data: vec![],
                        };
                        let mut tx_type = tx_with_pubkey.tx_type;

                        //check if pumpfun token has migrated or not, if true, switch to pumpswap sell logic
                        let rpc: &solana_client::rpc_client::RpcClient = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
                        let mut bonding_curve_state = BondingCurve::default();
                        
                        if tx_type == "pumpfun" {
                            if let Some(pump_fun_accounts) = &tx_with_pubkey.pump_fun_accounts {
                                bonding_curve_state = get_bonding_curve_state(pump_fun_accounts);
                                
                                if bonding_curve_state.complete {
                                    tx_type = "pump_swap".to_string();
                                    #[cfg(feature = "verbose_logging")]
                                    println!("[{}] - [grpc] Pumpfun token has migrated to pumpswap - applying pumpswap sell logic", now.format("%Y-%m-%d %H:%M:%S%.3f"));
                                    tx_with_pubkey.pump_swap_accounts = Some(GLOBAL_MONITORING_DATA.get(&tx_with_pubkey.mint).unwrap().pump_fun_accounts.clone());
                                    //need to figure out how to build pump swap struct!!!!!!!!!!!!!
                                }
                            }
                        }

                        if tx_type == "ray_launch" {
                            if let Some(ray_launch_accounts) = &tx_with_pubkey.ray_launch_accounts {
                                let pool_state = ray_launch_accounts.pool_state;
                                let rpc_start = Instant::now();
                                let res = match rpc.get_account_data(&pool_state) {
                                    Ok(data) => data,
                                    Err(e) => {
                                        eprintln!("[crossbeam_worker] Error: get_account_data (raylaunch) failed: {:?}", e);
                                        continue;
                                    }
                                };
                                let rpc_time = rpc_start.elapsed();
                                let status = res[17];
                                let migrate = res[20];
                                
                                if status > 0 {
                                    // tx_type = "ray_cpmm".to_string();
                                    if migrate == 1 {
                                        #[cfg(feature = "verbose_logging")]
                                        println!("[{}] - [grpc] Raylaunch pool is complete - applying Raydium CPMM sell logic", now.format("%Y-%m-%d %H:%M:%S%.3f"));
                                        tx_type = "ray_cpmm".to_string();
                                        tx_with_pubkey.raydium_cpmm_accounts = Some(GLOBAL_MONITORING_DATA.get(&tx_with_pubkey.mint).unwrap().ray_cpmm_accounts.clone());
                                    }
                                }
                            }
                        }

                        if tx_type == "pumpfun" {
                            if let Some(pump_fun_accounts) = &tx_with_pubkey.pump_fun_accounts {
                                sell_instruction = build_sell_instruction(
                                    tx_with_pubkey.token_amount,
                                    config.sell_slippage_bps,
                                    pump_fun_accounts,
                                    bonding_curve_state,
                                );
                                send_tx = true;
                            }
                        }
                        if tx_type == "pump_swap" {
                            if let Some(pump_swap_accounts) = &tx_with_pubkey.pump_swap_accounts {
                                sell_instruction = build_pump_sell_instruction(
                                    tx_with_pubkey.token_amount,
                                    config.sell_slippage_bps,
                                    pump_swap_accounts,
                                );
                                send_tx = true;
                            }
                        }
                        if tx_type == "ray_launch" {
                            if let Some(ray_launch_accounts) = &tx_with_pubkey.ray_launch_accounts {
                                sell_instruction = build_ray_launch_sell_instruction(
                                    tx_with_pubkey.token_amount,
                                    config.sell_slippage_bps,
                                    ray_launch_accounts,
                                );
                                send_tx = true;
                            }
                        }
                        if tx_type == "ray_cpmm" {
                            if let Some(raydium_cpmm_accounts) = &tx_with_pubkey.raydium_cpmm_accounts {
                                sell_instruction = build_ray_cpmm_sell_instruction(
                                    tx_with_pubkey.token_amount,
                                    raydium_cpmm_accounts,
                                );
                                send_tx = true;
                            }
                        }
                        if tx_type == "ray_launch_cpmm" {
                            if let Some(raydium_cpmm_accounts) = &tx_with_pubkey.raydium_cpmm_accounts {
                                sell_instruction = build_ray_cpmm_sell_instruction(
                                    tx_with_pubkey.token_amount,
                                    raydium_cpmm_accounts,
                                );
                                send_tx = true;
                            }
                        }

                        if send_tx {
                            let build_start = Instant::now();
                            #[cfg(feature = "verbose_logging")]
                            {
                                let now = Utc::now();
                                println!("[{}] - [TRITON] Building sell transaction for sig: {} (tx_type: {})", 
                                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect, tx_type);
                            }
                            
                            // Build vendor-specific sell transactions in parallel using the same function as buy
                            let build_result = crate::build_tx::tx_builder::build_vendor_specific_transactions_parallel(
                                sell_instruction,
                                tx_with_pubkey.mint,
                                0, // target_token_buy not used for sell transactions
                                &sig_detect, // sig_str for logging
                            );
                            let build_time = build_start.elapsed();
                            
                            match build_result {
                                Ok(vendor_transactions) => {
                                    if !vendor_transactions.is_empty() {
                                        #[cfg(feature = "verbose_logging")]
                                        {
                                            let now = Utc::now();
                                            println!("[{}] - [TRITON] SUCCESS - Built {} vendor sell transactions for sig: {}", 
                                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), vendor_transactions.len(), sig_detect);
                                        }
                                        
                                        // Send all vendor transactions in parallel
                                        let sig_detect_clone = sig_detect.clone();
                                        let sig_bytes_clone = sig_bytes.clone();
                                        let detection_time = parsed.detection_time.unwrap();
                                        
                                        let send_start = Instant::now();
                                        ASYNC_RUNTIME.spawn(async move {
                                            let send_result = send_all_vendors_parallel(&vendor_transactions, detection_time).await;
                                            let send_time = send_start.elapsed();
                                            
                                            match send_result {
                                                Ok((winning_vendor, sig)) => {
                                                    TRITON_TRANSACTIONS_SENT.fetch_add(1, Ordering::Relaxed);
                                                    #[cfg(feature = "verbose_logging")]
                                                    {
                                                        let now = Utc::now();
                                                        println!(
                                                            "[{}] - [TRITON] PARALLEL SELL SUCCESS - {} won with sig: {} | total sent: {}",
                                                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                                                            winning_vendor,
                                                            sig,
                                                            TRITON_TRANSACTIONS_SENT.load(Ordering::Relaxed)
                                                        );
                                                    }
                                                    // Remove the processed transaction from GLOBAL_TX_MAP to prevent memory leaks
                                                    GLOBAL_TX_MAP.remove(&sig_bytes_clone);
                                                }
                                                Err(e) => {
                                                    TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                                    #[cfg(feature = "verbose_logging")]
                                                    {
                                                        let now = Utc::now();
                                                        eprintln!("[{}] - [TRITON] ERROR - Parallel sell send failed for sig: {} - Error: {:?}", 
                                                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect_clone, e);
                                                    }
                                                }
                                            }
                                        });
                                    } else {
                                        TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                        #[cfg(feature = "verbose_logging")]
                                        {
                                            let now = Utc::now();
                                            eprintln!("[{}] - [TRITON] ERROR - No vendor sell transactions built for sig: {}", 
                                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect);
                                        }
                                    }
                                }
                                Err(e) => {
                                    TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                    #[cfg(feature = "verbose_logging")]
                                    {
                                        let now = Utc::now();
                                        eprintln!("[{}] - [TRITON] ERROR - Failed to build vendor sell transactions for sig: {} - Error: {:?}", 
                                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect, e);
                                    }
                                }
                            }
                        } else {
                            #[cfg(feature = "verbose_logging")]
                            {
                                let now = Utc::now();
                                println!("[{}] - [TRITON] No sell transaction to build for sig: {} (tx_type: {})", 
                                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect, tx_type);
                            }
                        }
                    }

                } else { //send tx
                    let else_branch_start = Instant::now();
                    #[cfg(feature = "verbose_logging")]
                    {
                        let now = Utc::now();
                        println!("[{}] - [TRITON] Attempting to send buy transaction for sig: {}", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect);
                    }
                    
                    let sig_bytes_check_start = Instant::now();
                    let sig_bytes_check = parsed.sig_bytes.is_some();
                    let sig_bytes_check_time = sig_bytes_check_start.elapsed();
                    
                    if let Some(sig_bytes) = &parsed.sig_bytes {
                        let map_get_start = Instant::now();
                        let map_get_result = GLOBAL_TX_MAP.get_mut(sig_bytes);
                        let map_get_time = map_get_start.elapsed();
                        
                        if let Some(mut tx_with_pubkey) = map_get_result {
                            // Get vendor transactions for parallel sending
                            let vendor_transactions = tx_with_pubkey.vendor_transactions.clone();
                            let detection_time = parsed.detection_time.unwrap();
                            let slot = parsed.slot.unwrap();
                            let sig_bytes_clone = sig_bytes.clone();
                            
                            // Update the transaction info immediately (non-blocking) - set send_slot agnostic to which vendor wins
                            tx_with_pubkey.send_time = Instant::now();
                            tx_with_pubkey.send_slot = slot; // Set send_slot immediately when we start sending
                            
                            let buy_send_start = Instant::now();
                            ASYNC_RUNTIME.spawn(async move {
                                let buy_send_result = send_all_vendors_parallel(&vendor_transactions, detection_time).await;
                                let buy_send_time = buy_send_start.elapsed();
                                
                                match buy_send_result {
                                    Ok((winning_vendor, sig)) => {
                                        TRITON_TRANSACTIONS_SENT.fetch_add(1, Ordering::Relaxed);
                                        
                                        let now = Utc::now();
                                        println!(
                                            "[{}] - [TRITON] PARALLEL SUCCESS - {} won with sig: {} | total sent: {}",
                                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                                            winning_vendor,
                                            sig,
                                            TRITON_TRANSACTIONS_SENT.load(Ordering::Relaxed)
                                        );
                                        
                                        // Update only the signature in the map (send_slot already set above)
                                        if let Some(mut tx_with_pubkey) = GLOBAL_TX_MAP.get_mut(&sig_bytes_clone) {
                                            tx_with_pubkey.send_sig = sig.clone();
                                            // send_slot is already set above, so we don't need to set it again
                                            #[cfg(feature = "verbose_logging")]
                                            {
                                                let now = Utc::now();
                                                println!("[{}] - [TRITON] Saved sig: {} to GLOBAL_TX_MAP (send_slot already set)", Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                        #[cfg(feature = "verbose_logging")]
                                        {
                                            let now = Utc::now();
                                            eprintln!("[crossbeam_worker] Error: Parallel send failed: {:?}", e);
                                        }
                                    }
                                }
                            });
                        } else {
                            TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                            #[cfg(feature = "verbose_logging")]
                            {
                                let now = Utc::now();
                                eprintln!("[crossbeam_worker] Error: GLOBAL_TX_MAP.get_mut failed for sig_detect={}", sig_detect);
                            }
                        }
                    } else {
                        TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                        #[cfg(feature = "verbose_logging")]
                        {
                            let now = Utc::now();
                            eprintln!("[crossbeam_worker] Error: No sig_bytes found for sig_detect={}", sig_detect);
                        }
                    }
                }
                
                // OPTIMIZATION: Track processing time
                let processing_time = processing_start.elapsed();
                let processing_time_micros = processing_time.as_micros() as usize;
                TRITON_TOTAL_PROCESSING_TIME.fetch_add(processing_time_micros, Ordering::Relaxed);
                TRITON_PROCESSING_TIMES.fetch_add(1, Ordering::Relaxed);
                
                // OPTIMIZATION: Log detailed profiling for all messages
                let receive_time = receive_start.elapsed();
                #[cfg(feature = "verbose_logging")]
                {
                    println!("[TRITON-{}] PROFILE - receive: {:.2?}, sig_extract: {:.2?}, is_signer: {:.2?}, map_size: {:.2?}, map_search: {:.2?}, found_check: {:.2?}, sig_bytes_check: {:.2?}, map_get: {:.2?}, rpc: {:.2?}, wait: {:.2?}, build: {:.2?}, send: {:.2?}, buy_send: {:.2?}, total: {:.2?} for sig: {}", 
                        worker_id, receive_time, sig_extract_time, is_signer_check_time, map_size_time, map_search_time, found_check_time, sig_bytes_check_time, map_get_time, rpc_time, wait_time, build_time, send_time, buy_send_time, processing_time, sig_detect);
                }
                
                // OPTIMIZATION: Log slow processing with detailed breakdown
                if processing_time_micros > 1000 { // > 1ms
                    eprintln!("[TRITON-{}] SLOW PROCESSING: {}µs for sig: {} (sig_extract: {:.2?}, map_search: {:.2?}, wait: {:.2?})", 
                        worker_id, processing_time_micros, sig_detect, sig_extract_time, map_search_time, wait_time);
                }
            }
        });
    }
}

/// Call this from your parser to send a parsed message to the worker.
pub fn send_parsed_tx(parsed: ParsedTx) {
    let send_start = std::time::Instant::now();
    let sig_string = parsed.sig_bytes.as_ref().map(|s| bs58::encode(s).into_string()).unwrap_or_default();
    
    if let Some(sender) = PARSED_TX_SENDER.get() {
        let send_result = sender.send(parsed);
        let send_time = send_start.elapsed();
        
        #[cfg(feature = "verbose_logging")]
        {
            match send_result {
                Ok(_) => {
                    println!("[TRITON] CHANNEL SEND SUCCESS: {:.2?} for sig: {}", send_time, sig_string);
                }
                Err(e) => {
                    eprintln!("[TRITON] CHANNEL SEND FAILED: {:.2?} for sig: {} - Error: {:?}", send_time, sig_string, e);
                }
            }
        }
    } else {
        let send_time = send_start.elapsed();
        eprintln!("[TRITON] CHANNEL SEND ERROR: {:.2?} - No sender available", send_time);
    }
} 




// pub async fn process_triton_message(resp: &SubscribeUpdate) {
//     let config = GLOBAL_CONFIG.get().expect("Config not initialized");

//     if let Some(update) = &resp.update_oneof {
//         match update {
//             UpdateOneof::Transaction(tx_update) => {
//                 if let Some(tx_info) = &tx_update.transaction {
//                     if let Some(tx) = &tx_info.transaction {
//                         let sig_bytes = tx.signatures.get(0).map(|s| s.clone());
//                         let wallet_pubkey = get_wallet_keypair().pubkey();
//                         let is_signer = if let Some(message) = &tx.message {
//                             if let Some(header) = &message.header {
//                                 let num_signers = header.num_required_signatures as usize;
//                                 message.account_keys
//                                     .iter()
//                                     .take(num_signers)
//                                     .any(|bytes| {
//                                         let pubkey = unsafe {
//                                             Pubkey::new_from_array(*(bytes.as_ptr() as *const [u8; 32]))
//                                         };
//                                         pubkey == wallet_pubkey
//                                     })
//                             } else { false }
//                         } else { false };
//                         // Handoff to crossbeam worker for heavy processing
//                         let parsed = ParsedTx {
//                             sig_bytes,
//                             is_signer,
//                             slot: Some(tx_update.slot),
//                         };
//                         send_parsed_tx(parsed);
//                         return;
//                     }
//                 }
//             }
//             // The following arms are commented out for speed. Uncomment if needed for debugging or additional features.
//             // UpdateOneof::Slot(slot_info) => {
//             //     let now = Utc::now();
//             //     println!(
//             //         "[{}] - [Triton] Slot Update: slot={}, status={:?}",
//             //         now.format("%Y-%m-%d %H:%M:%S%.3f"),
//             //         slot_info.slot,
//             //         slot_info.status()
//             //     );
//             // }
//             // UpdateOneof::Account(account_info) => {
//             //     let now = Utc::now();
//             //     println!(
//             //         "[{}] - [Triton] Account Update: {:?}",
//             //         now.format("%Y-%m-%d %H:%M:%S%.3f"),
//             //         account_info.account
//             //     );
//             // }
//             // UpdateOneof::Block(block_info) => {
//             //     // let now = Utc::now();
//             //     // println!("[{}] - [Triton] Block Update: slot={}", now.format("%Y-%m-%d %H:%M:%S%.3f"), block_info.slot);
//             // }
//             // UpdateOneof::Ping(_) => {
//             //     // let now = Utc::now();
//             //     // println!(
//             //     //     "[{}] - [Triton] Received ping.",
//             //     //     now.format("%Y-%m-%d %H:%M:%S%.3f")
//             //     // );
//             // }
//             // UpdateOneof::Pong(_) => {
//             //     // let now = Utc::now();
//             //     // println!(
//             //     //     "[{}] - [Triton] Received pong.",
//             //     //     now.format("%Y-%m-%d %H:%M:%S%.3f")
//             //     // );
//             // }
//             // _ => {
//             //     let now = Utc::now();
//             //     println!(
//             //         "[{}] - [Triton] Received other message type: {:?}",
//             //         now.format("%Y-%m-%d %H:%M:%S%.3f"),
//             //         update
//             //     );
//             // }
//             _ => {}
//         }
//     }
// }
