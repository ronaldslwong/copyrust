use crossbeam::channel::{bounded, Sender};
use once_cell::sync::OnceCell;
use bs58;
use core_affinity;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::Instruction;
use chrono::Utc;
use crate::utils::logger::{log_event, EventType};
use crate::utils::rt_scheduler::{set_realtime_priority, RealtimePriority};
use dashmap::DashMap;
use once_cell::sync::Lazy;

// use tokio::time::{sleep, Duration};
// REMOVED: use crate::grpc::arpc_worker::GLOBAL_TX_MAP; - ARPC is decommissioned
use crate::build_tx::pump_fun::{build_sell_instruction, get_bonding_curve_state, BondingCurve};
use crate::build_tx::pump_swap::build_pump_sell_instruction;

use crate::build_tx::ray_launch::build_ray_launch_sell_instruction;
use crate::build_tx::ray_cpmm::{build_ray_cpmm_sell_instruction};
use crate::build_tx::heaven::build_heaven_sell_instruction;
use crate::build_tx::boop_fun::build_boop_fun_sell_instruction;
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

// NEW: Global transaction storage for buy transactions

// Structure to store transaction information for sell transactions
#[derive(Debug, Clone)]
pub struct TransactionInfo {
    pub send_sig: String,           // Signature of the transaction we sent
    pub send_slot: u64,             // Slot when we sent the transaction
    pub send_time: Instant,         // Time when we sent the transaction
    pub tx_type: String,            // Type of transaction (pumpfun, ray_launch, etc.)
    pub mint: Pubkey,               // Mint address of the token
    pub token_amount: u64,          // Amount of tokens we bought
    pub sol_amount: u64,            // Amount of SOL we spent
    
    // NEW: Nonce tracking for blockhash refresh
    pub nonce_index: Option<usize>, // Index of the nonce account used
    pub nonce_pubkey: Option<Pubkey>, // Pubkey of the nonce account used
    
    // Program-specific account structures
    pub pump_fun_accounts: Option<crate::build_tx::pump_fun::PumpFunAccounts>,
    pub pump_swap_accounts: Option<crate::build_tx::pump_swap::PumpAmmAccounts>,
    pub ray_launch_accounts: Option<crate::build_tx::ray_launch::RayLaunchAccounts>,
    pub raydium_cpmm_accounts: Option<crate::build_tx::ray_cpmm::RayCpmmSwapAccounts>,
    pub heaven_accounts: Option<crate::build_tx::heaven::HeavenAccounts>,
    pub boop_fun_accounts: Option<crate::build_tx::boop_fun::BoopFunAccounts>,
    
    // Additional metadata
    pub detection_time: Instant,    // When we first detected the transaction
    pub feed_id: String,            // Which feed detected it
    pub instructions: Option<Vec<Instruction>>, // Original instructions
    pub account_keys: Option<Vec<Pubkey>>,      // Account keys from original transaction
}

// Global storage for transaction information (lock-free)
static GLOBAL_TX_MAP: Lazy<DashMap<Vec<u8>, TransactionInfo>> = Lazy::new(|| {
    DashMap::new()
});

// Constants
const MAX_CONSECUTIVE_ERRORS: usize = 5;

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

/// Store transaction information with nonce tracking
pub fn store_transaction_info_with_nonce(
    signature: Vec<u8>,
    slot: u64,
    tx_type: String,
    mint: Pubkey,
    token_amount: u64,
    sol_amount: u64,
    pump_fun_accounts: Option<crate::build_tx::pump_fun::PumpFunAccounts>,
    pump_swap_accounts: Option<crate::build_tx::pump_swap::PumpAmmAccounts>,
    ray_launch_accounts: Option<crate::build_tx::ray_launch::RayLaunchAccounts>,
    raydium_cpmm_accounts: Option<crate::build_tx::ray_cpmm::RayCpmmSwapAccounts>,
    heaven_accounts: Option<crate::build_tx::heaven::HeavenAccounts>,
    boop_fun_accounts: Option<crate::build_tx::boop_fun::BoopFunAccounts>,
    detection_time: Instant,
    feed_id: String,
    instructions: Option<Vec<Instruction>>,
    account_keys: Option<Vec<Pubkey>>,
    nonce_index: Option<usize>,
    nonce_pubkey: Option<Pubkey>,
) {
    let tx_info = TransactionInfo {
        send_sig: bs58::encode(&signature).into_string(),
        send_slot: slot,
        send_time: Instant::now(),
        tx_type: tx_type.clone(),
        mint,
        token_amount,
        sol_amount,
        nonce_index,        // NEW: Track nonce index
        nonce_pubkey,       // NEW: Track nonce pubkey
        pump_fun_accounts,
        pump_swap_accounts,
        ray_launch_accounts,
        raydium_cpmm_accounts,
        heaven_accounts,
        boop_fun_accounts,
        detection_time,
        feed_id,
        instructions,
        account_keys,
    };
    
    GLOBAL_TX_MAP.insert(signature.clone(), tx_info);
    
    let sig_string = bs58::encode(&signature).into_string();
    println!("[TRITON] Stored transaction info for signature: {} (type: {}, mint: {}, nonce_index: {:?})", 
        sig_string, tx_type, mint, nonce_index);
}

/// Store transaction information (legacy function - now calls the new one)
pub fn store_transaction_info(
    signature: Vec<u8>,
    slot: u64,
    tx_type: String,
    mint: Pubkey,
    token_amount: u64,
    sol_amount: u64,
    pump_fun_accounts: Option<crate::build_tx::pump_fun::PumpFunAccounts>,
    pump_swap_accounts: Option<crate::build_tx::pump_swap::PumpAmmAccounts>,
    ray_launch_accounts: Option<crate::build_tx::ray_launch::RayLaunchAccounts>,
    raydium_cpmm_accounts: Option<crate::build_tx::ray_cpmm::RayCpmmSwapAccounts>,
    heaven_accounts: Option<crate::build_tx::heaven::HeavenAccounts>,
    boop_fun_accounts: Option<crate::build_tx::boop_fun::BoopFunAccounts>,
    detection_time: Instant,
    feed_id: String,
    instructions: Option<Vec<Instruction>>,
    account_keys: Option<Vec<Pubkey>>,
) {
    store_transaction_info_with_nonce(
        signature,
        slot,
        tx_type,
        mint,
        token_amount,
        sol_amount,
        pump_fun_accounts,
        pump_swap_accounts,
        ray_launch_accounts,
        raydium_cpmm_accounts,
        heaven_accounts,
        boop_fun_accounts,
        detection_time,
        feed_id,
        instructions,
        account_keys,
        None, // nonce_index
        None, // nonce_pubkey
    )
}

// NEW: Function to retrieve transaction information by signature
pub fn get_transaction_info(signature: &[u8]) -> Option<TransactionInfo> {
    GLOBAL_TX_MAP.get(signature).map(|entry| entry.value().clone())
}

// NEW: Function to remove transaction information after processing
pub fn remove_transaction_info(signature: &[u8]) {
    if GLOBAL_TX_MAP.remove(signature).is_some() {
        let sig_string = bs58::encode(signature).into_string();
        println!("[TRITON] Removed transaction info for signature: {}", sig_string);
    }
}

// NEW: Function to get transaction info by signature string
pub fn get_transaction_info_by_sig_string(sig_string: &str) -> Option<TransactionInfo> {
    // Convert signature string back to bytes for lookup
    if let Ok(sig_bytes) = bs58::decode(sig_string).into_vec() {
        get_transaction_info(&sig_bytes)
    } else {
        None
    }
}

/// Initialize the nonce refresh channel and background task
pub fn initialize_nonce_refresh_system() {
    let (sender, receiver) = mpsc::channel::<String>();
    
    // Store the sender for use by other parts of the system
    if NONCE_REFRESH_SENDER.set(sender).is_err() {
        eprintln!("[TRITON] Failed to initialize nonce refresh sender - already initialized");
        return;
    }
    
    // Spawn background task to handle nonce refresh requests
    std::thread::spawn(move || {
        // Create a Tokio runtime for async operations
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime for nonce refresh");
        
        loop {
            match receiver.recv() {
                Ok(signature) => {
                    // Handle the nonce refresh in async context
                    rt.block_on(async {
                        handle_nonce_refresh_request(&signature).await;
                    });
                }
                Err(e) => {
                    eprintln!("[TRITON] Nonce refresh channel closed: {}", e);
                    break;
                }
            }
        }
    });
    
    println!("[TRITON] Nonce refresh system initialized");
}

/// Trigger nonce blockhash refresh when we detect our own transaction (non-async version)
pub fn trigger_nonce_refresh_for_transaction(signature: &str) {
    if let Some(sender) = NONCE_REFRESH_SENDER.get() {
        if let Err(e) = sender.send(signature.to_string()) {
            eprintln!("[TRITON] Failed to send nonce refresh request: {}", e);
        }
    } else {
        eprintln!("[TRITON] Nonce refresh sender not initialized");
    }
}

/// Handle nonce refresh request in async context
async fn handle_nonce_refresh_request(signature: &str) {
    // Look up the transaction in our global map to get nonce information
    if let Some(tx_info) = get_transaction_info_by_sig_string(signature) {
        if let (Some(nonce_index), Some(nonce_pubkey)) = (tx_info.nonce_index, tx_info.nonce_pubkey) {
            println!("[TRITON] Processing nonce refresh for signature: {} (nonce_index: {}, nonce_pubkey: {})", 
                signature, nonce_index, nonce_pubkey);
            
            if let Some(rpc_client) = crate::init::initialize::GLOBAL_RPC_CLIENT.get() {
                // Fetch new blockhash for this nonce using the synchronous function
                if let Ok(blockhash) = crate::init::wallet_loader::fetch_nonce_blockhash_sync(rpc_client, &nonce_pubkey) {
                    // Update the cache
                    if let Err(e) = crate::init::wallet_loader::update_nonce_blockhash(&nonce_pubkey, blockhash).await {
                        eprintln!("[TRITON] Failed to update nonce blockhash: {}", e);
                    } else {
                        println!("[TRITON] Successfully refreshed nonce {} blockhash: {}", nonce_index, blockhash);
                    }
                } else {
                    eprintln!("[TRITON] Failed to fetch new blockhash for nonce {}", nonce_index);
                }
            } else {
                eprintln!("[TRITON] RPC client not available for nonce refresh");
            }
            
            // Remove the transaction from our map since we've processed it
            if let Ok(sig_bytes) = bs58::decode(signature).into_vec() {
                remove_transaction_info(&sig_bytes);
            }
        } else {
            println!("[TRITON] No nonce information found for signature: {}", signature);
        }
    } else {
        println!("[TRITON] Transaction not found in global map for signature: {}", signature);
    }
}

// Create a global Tokio runtime for async operations
pub static ASYNC_RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Runtime::new().expect("Failed to create async runtime")
});

#[derive(Debug, Clone)]
pub struct SimpleTokenBalance {
    pub mint: String,
    pub ui_amount: f64,
    pub decimals: u32,
    pub amount: String,
    pub owner: String,
    pub program_id: String,
}

#[derive(Debug, Clone)]
pub struct ParsedTx {
    pub sig_bytes: Option<Vec<u8>>,
    pub is_signer: bool,
    pub slot: Option<u64>,
    pub detection_time: Option<Instant>,
    pub feed_id: String, // OPTIMIZATION: Add feed identification
    pub pre_token_balances: Option<Vec<SimpleTokenBalance>>,
    pub post_token_balances: Option<Vec<SimpleTokenBalance>>,
    
    // NEW FIELDS for transaction building
    pub instructions: Option<Vec<solana_sdk::instruction::Instruction>>,
    pub account_keys: Option<Vec<solana_sdk::pubkey::Pubkey>>,
    pub recent_blockhash: Option<solana_sdk::hash::Hash>,
    pub fee_payer: Option<solana_sdk::pubkey::Pubkey>,
    pub token_amount_change: Option<i64>, // Store parsed token amount change
    
    // NEW: SOL buy amount and mint token amount for program instructions
    pub sol_buy_amount_lamports: Option<u64>, // SOL amount to buy in lamports
    pub mint_token_amount: Option<u64>, // Mint token amount from instruction data
    
    // PERFORMANCE TRACKING: Timing information for performance analysis
    pub parser_first_hit: Option<std::time::Instant>, // When we first hit the parser
    pub parse_time: Option<std::time::Duration>, // Time spent parsing the transaction
    
    // Add more fields as needed
}

// Note: ParsedTxWithTokenBalances removed to avoid type conflicts
// We'll use the existing RPC-based approach for now

// OPTIMIZATION: Global deduplication for multiple feeds
use std::collections::HashMap;
use std::sync::mpsc;

// Channel for sending nonce refresh requests
static NONCE_REFRESH_SENDER: OnceCell<mpsc::Sender<String>> = OnceCell::new();

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

use std::sync::Arc;

// Worker health tracking
static WORKER_HEALTH: OnceCell<Arc<DashMap<usize, (Instant, bool)>>> = OnceCell::new();

static ACTIVE_WORKERS: AtomicUsize = AtomicUsize::new(0);
static WORKER_RESTART_COUNT: AtomicUsize = AtomicUsize::new(0);

// Worker restart configuration
const MIN_WORKERS: usize = 6;
const MAX_WORKER_RESTARTS: usize = 100;
const WORKER_HEALTH_CHECK_INTERVAL_SECS: u64 = 5;

/// Spawn a single worker thread with restart capability
fn spawn_worker(worker_id: usize, rx: crossbeam::channel::Receiver<ParsedTx>) -> std::thread::JoinHandle<()> {
    let rx_clone = rx.clone();
    let health_tracker = WORKER_HEALTH.get_or_init(|| Arc::new(DashMap::new())).clone();
    
    // Mark worker as active
    ACTIVE_WORKERS.fetch_add(1, Ordering::Relaxed);
    
    std::thread::spawn(move || {
        // Set worker health status
        health_tracker.insert(worker_id, (Instant::now(), true));
        
        println!("[TRITON] Worker thread {} started and waiting for messages...", worker_id);
        
        // Pin worker threads to cores 2-7 for optimal performance
        if let Some(cores) = core_affinity::get_core_ids() {
            if cores.len() > 2 + worker_id {
                core_affinity::set_for_current(cores[2 + worker_id]);
                println!("[TRITON] Worker {} pinned to core {}", worker_id, 2 + worker_id);
            }
        }
        
        // Set critical real-time priority for processing (highest priority)
        if let Err(e) = set_realtime_priority(RealtimePriority::Critical) {
            eprintln!("[triton crossbeam worker {}] Failed to set real-time priority: {}", worker_id, e);
        }
        
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: usize = 10;
        
        println!("[TRITON] Worker {} entering main message loop", worker_id);
        
        // Main worker loop
        let result = worker_loop(worker_id, rx_clone, &mut consecutive_errors, MAX_CONSECUTIVE_ERRORS);
        
        // Mark worker as inactive
        if let Some(mut entry) = health_tracker.get_mut(&worker_id) {
            entry.1 = false;
        }
        ACTIVE_WORKERS.fetch_sub(1, Ordering::Relaxed);
        
        println!("[TRITON] Worker {} exiting - result: {:?}", worker_id, result);
        result
    })
}

/// Main worker processing loop - extracted for restart capability
fn worker_loop(
    worker_id: usize, 
    rx: crossbeam::channel::Receiver<ParsedTx>, 
    consecutive_errors: &mut usize, 
    max_errors: usize
) -> () {
    while let Ok(parsed) = rx.recv() {
        println!("[TRITON] Worker {} received message for sig: {}", worker_id, 
            parsed.sig_bytes.as_ref().map(|s| bs58::encode(s).into_string()).unwrap_or_default());
        println!("[TRITON] Worker {} DEBUG - Received ParsedTx with mint_token_amount: {:?}", worker_id, parsed.mint_token_amount);
        
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
        
        // Initialize transaction tracking variables
        let mut found: Option<TransactionInfo> = None;

        if parsed.detection_time.is_none() {
            *consecutive_errors += 1;
            TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
            eprintln!("[crossbeam_worker] Error: detection_time is None for sig_detect={}", sig_detect);
            
            if *consecutive_errors >= max_errors {
                eprintln!("[Triton worker {}] Too many consecutive errors ({}), will restart...", worker_id, *consecutive_errors);
                return;
            }
            continue;
        }
        
        *consecutive_errors = 0; // Reset error counter on successful processing

        let config = match GLOBAL_CONFIG.get() {
            Some(cfg) => cfg,
            None => {
                eprintln!("[crossbeam_worker] Error: Config not initialized");
                *consecutive_errors += 1;
                if *consecutive_errors >= max_errors {
                    return;
                }
                continue;
            }
        };
        
        let is_signer_check_start = Instant::now();
        let is_signer = parsed.is_signer;
        let is_signer_check_time = is_signer_check_start.elapsed();
        
        println!("[TRITON] Worker {} processing transaction - is_signer: {}", worker_id, is_signer);
        
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
            let mut token_2022 = false;
            
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
                                *consecutive_errors += 1;
                                if *consecutive_errors >= max_errors {
                                    return;
                                }
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
                    println!("[pumpfun]: {:?}", tx_with_pubkey.pump_fun_accounts);
                    println!("[pumpfun] DEBUG - Using token_amount: {} (stored) vs mint_token_amount: {:?} (parsed)", 
                        tx_with_pubkey.token_amount, parsed.mint_token_amount);
                    if let Some(pump_fun_accounts) = &tx_with_pubkey.pump_fun_accounts {
                        sell_instruction = build_sell_instruction(
                            parsed.mint_token_amount.unwrap_or(0),  // Use parsed amount (actual balance change)
                            config.sell_slippage_bps,
                            pump_fun_accounts,
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
                if tx_type == "heaven" {
                    if let Some(heaven_accounts) = &tx_with_pubkey.heaven_accounts {
                        sell_instruction = build_heaven_sell_instruction(
                            parsed.mint_token_amount.unwrap_or(0),
                            heaven_accounts,
                        );
                        send_tx = true;
                        token_2022 = true;
                    }
                }
                if tx_type == "boop_fun" {
                    if let Some(boop_fun_accounts) = &tx_with_pubkey.boop_fun_accounts {
                        sell_instruction = build_boop_fun_sell_instruction(
                            parsed.mint_token_amount.unwrap_or(0),
                            boop_fun_accounts,
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
                        token_2022,
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
                                
                                // PERFORMANCE TRACKING: Record when we start sending sell transactions
                                let send_start = Instant::now();
                                ASYNC_RUNTIME.spawn(async move {
                                    let send_result = send_all_vendors_parallel(&vendor_transactions, detection_time).await;
                                    let send_time = send_start.elapsed();
                                    
                                    // PERFORMANCE TRACKING: Log sell transaction performance
                                    println!("[TRITON] PERFORMANCE - Sell TX Performance for sig: {}: Send Time: {:.2?}", sig_detect_clone, send_time);
                                    
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
            println!("[TRITON] Worker {} - Transaction is NOT a signer, calling build_and_send_tx", worker_id);
            let else_branch_start = Instant::now();
            
            // NEW: Use build_and_send_tx instead of old ARPC logic
            crate::triton_grpc::build_send_tx::build_and_send_tx(parsed.clone());
            
            let else_branch_time = else_branch_start.elapsed();
            println!("[TRITON] Worker {} - build_and_send_tx completed in {:.2?}", worker_id, else_branch_time);
        }
        
        // OPTIMIZATION: Track processing time
        let processing_time = processing_start.elapsed();
        let processing_time_micros = processing_time.as_micros() as usize;
        TRITON_TOTAL_PROCESSING_TIME.fetch_add(processing_time_micros, Ordering::Relaxed);
        TRITON_PROCESSING_TIMES.fetch_add(1, Ordering::Relaxed);
        
        println!("[TRITON] Worker {} - Message processing completed in {:.2?}", worker_id, processing_time);
        
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
    
    ()
}

/// Worker health monitoring and restart task
fn worker_health_monitor(rx: crossbeam::channel::Receiver<ParsedTx>) {
    let mut worker_handles: Vec<(usize, std::thread::JoinHandle<()>)> = Vec::new();
    
    // Spawn initial workers
    for worker_id in 0..MIN_WORKERS {
        let handle = spawn_worker(worker_id, rx.clone());
        worker_handles.push((worker_id, handle));
    }
    
    println!("[TRITON] Initial {} workers spawned", MIN_WORKERS);
    
    loop {
        std::thread::sleep(Duration::from_secs(WORKER_HEALTH_CHECK_INTERVAL_SECS));
        
        // Check worker health and restart dead workers
        let mut to_restart = Vec::new();
        let mut active_count = 0;
        
        for (worker_id, handle) in &mut worker_handles {
            if handle.is_finished() {
                to_restart.push(*worker_id);
            } else {
                active_count += 1;
            }
        }
        
        // Restart dead workers
        for worker_id in to_restart {
            if WORKER_RESTART_COUNT.load(Ordering::Relaxed) < MAX_WORKER_RESTARTS {
                println!("[TRITON] Restarting dead worker {}", worker_id);
                WORKER_RESTART_COUNT.fetch_add(1, Ordering::Relaxed);
                
                // Remove old handle and spawn new worker
                worker_handles.retain(|(id, _)| *id != worker_id);
                let handle = spawn_worker(worker_id, rx.clone());
                worker_handles.push((worker_id, handle));
                
                println!("[TRITON] Worker {} restarted successfully", worker_id);
            } else {
                eprintln!("[TRITON] ERROR: Maximum worker restarts reached ({}), not restarting worker {}", 
                    MAX_WORKER_RESTARTS, worker_id);
            }
        }
        
        // Log worker status
        let total_workers = worker_handles.len();
        println!("[TRITON] Worker health check: {}/{} workers active, {} total restarts", 
            active_count, total_workers, WORKER_RESTART_COUNT.load(Ordering::Relaxed));
        
        // Emergency worker spawning if too many are dead
        if active_count < MIN_WORKERS / 2 {
            eprintln!("[TRITON] WARNING: Too many workers dead ({}), spawning emergency workers", active_count);
            
            for i in 0..(MIN_WORKERS - active_count) {
                let new_worker_id = total_workers + i;
                let handle = spawn_worker(new_worker_id, rx.clone());
                worker_handles.push((new_worker_id, handle));
                println!("[TRITON] Emergency worker {} spawned", new_worker_id);
            }
        }
    }
}

/// Call this once at startup (e.g., in main.rs) to spawn the worker thread.
/// This will create a pool of 6 worker threads that process parsed transactions.
/// The workers are pinned to CPU cores 2-7 for optimal performance.
/// Core allocation: 2,3,4,5,6,7 (expanded from cores 2-4 to include decommissioned ARPC cores)
pub fn setup_crossbeam_worker() {
    println!("[TRITON] setup_crossbeam_worker() called - starting setup...");
    let (tx, rx) = bounded::<ParsedTx>(2000); // Increased capacity to handle bursts
    println!("[TRITON] Channel created with capacity 2000");
    
    match PARSED_TX_SENDER.set(tx) {
        Ok(_) => println!("[TRITON] Crossbeam worker initialized"),
        Err(_) => {
            eprintln!("[TRITON] ERROR: PARSED_TX_SENDER already set!");
            return;
        }
    }
    
    // Start deduplication cleanup task
    std::thread::spawn(|| {
        println!("[TRITON] Deduplication cleanup task started");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            cleanup_feed_dedup_map();
        }
    });
    
    // Start worker health monitoring in separate thread
    std::thread::spawn(move || {
        println!("[TRITON] Worker health monitor started");
        worker_health_monitor(rx);
    });
    
    println!("[TRITON] Crossbeam worker setup completed successfully!");
    
    // Test the channel with a dummy message to verify it's working
    println!("[TRITON] Testing channel with dummy message...");
    if let Some(sender) = PARSED_TX_SENDER.get() {
        let dummy_tx = ParsedTx {
            sig_bytes: Some(vec![0u8; 32]),
            is_signer: false,
            slot: Some(0),
            detection_time: Some(Instant::now()),
            feed_id: "test".to_string(),
            pre_token_balances: None,
            post_token_balances: None,
            instructions: None,
            account_keys: None,
            recent_blockhash: None,
            fee_payer: None,
            token_amount_change: None,
            sol_buy_amount_lamports: None,
            mint_token_amount: None,
            parser_first_hit: None,
            parse_time: None,
        };
        
        match sender.try_send(dummy_tx) {
            Ok(_) => println!("[TRITON] Channel test successful - dummy message sent"),
            Err(e) => eprintln!("[TRITON] Channel test failed - dummy message failed: {:?}", e),
        }
    } else {
        eprintln!("[TRITON] ERROR: PARSED_TX_SENDER not available for testing");
    }
}

/// Call this from your parser to send a parsed message to the worker.
pub fn send_parsed_tx(parsed: ParsedTx) {
    let send_start = std::time::Instant::now();
    let sig_string = parsed.sig_bytes.as_ref().map(|s| bs58::encode(s).into_string()).unwrap_or_default();
    
    println!("[TRITON] send_parsed_tx called for sig: {} (feed: {})", sig_string, parsed.feed_id);
    
    if let Some(sender) = PARSED_TX_SENDER.get() {
        println!("[TRITON] Sender found, attempting to send message...");
        // Use regular send since workers are running and waiting for messages
        let send_result = sender.send(parsed.clone());
        let send_time = send_start.elapsed();
        
        // Log both success and failure for debugging
        match send_result {
            Ok(_) => {
                println!("[TRITON] CHANNEL SEND SUCCESS: {:.2?} for sig: {} (feed: {})", send_time, sig_string, parsed.feed_id);
            }
            Err(e) => {
                eprintln!("[TRITON] CHANNEL SEND FAILED: {:.2?} for sig: {} (feed: {}) - Error: {:?}", send_time, sig_string, parsed.feed_id, e);
                
                // Check if channel is full
                if let Some(sender) = PARSED_TX_SENDER.get() {
                    eprintln!("[TRITON] Channel send failed - this usually means the channel is full or workers are not consuming");
                }
            }
        }
    } else {
        let send_time = send_start.elapsed();
        eprintln!("[TRITON] CHANNEL SEND ERROR: {:.2?} - No sender available for sig: {} (feed: {})", send_time, sig_string, parsed.feed_id);
    }
} 
