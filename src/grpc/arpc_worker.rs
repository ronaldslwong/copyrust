use crossbeam::channel::{unbounded, Sender};
use crate::arpc::CompiledInstruction;
use once_cell::sync::OnceCell;
use std::time::Instant;
use core_affinity;
use crate::utils::rt_scheduler::{set_realtime_priority, RealtimePriority};
use solana_sdk::transaction::Transaction;
use solana_sdk::pubkey::Pubkey;
use crate::build_tx::ray_launch::RayLaunchAccounts;
use crate::build_tx::ray_cpmm::RaydiumCpmmPoolState;
use crate::build_tx::pump_swap::PumpAmmAccounts;
use crate::build_tx::pump_fun::PumpFunAccounts;
use crate::build_tx::ray_cpmm::RayCpmmSwapAccounts;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::Arc;
use crate::config_load::GLOBAL_CONFIG;
use crate::build_tx::tx_builder::{default_instruction, build_and_sign_transaction, simulate_transaction};
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use crate::build_tx::tx_builder::create_instruction;
use crate::init::wallet_loader::get_wallet_keypair;
use chrono::Utc;
use crate::send_tx::zero_slot::create_instruction_zeroslot;
use crate::constants::axiom::{AXIOM_PUMP_SWAP_PROGRAM_ID_BYTES, AXIOM_PUMP_FUN_PROGRAM_ID_BYTES};
use crate::constants::raydium_launchpad::RAYDIUM_LAUNCHPAD_PROGRAM_ID_BYTES;
use crate::constants::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID_BYTES;
use crate::grpc::programs::raydium_launchpad::raydium_launchpad_build_buy_tx;
use crate::grpc::programs::axiom::axiom_pump_swap_build_buy_tx;
use crate::grpc::programs::axiom::axiom_pump_fun_build_buy_tx;
use crate::grpc::programs::raydium_cpmm::raydium_cpmm_build_buy_tx;
use crate::build_tx::tx_builder::build_and_sign_transaction_fast;
use crate::send_tx::jito::create_instruction_jito;
use std::collections::HashMap;

// Add global counters for monitoring worker performance
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::atomic::AtomicU64;
static WORKER_MESSAGES_RECEIVED: AtomicUsize = AtomicUsize::new(0);
static WORKER_TRANSACTIONS_BUILT: AtomicUsize = AtomicUsize::new(0);
static WORKER_TRANSACTIONS_INSERTED: AtomicUsize = AtomicUsize::new(0);
static WORKER_ERRORS: AtomicUsize = AtomicUsize::new(0);

// Global performance counters
static STORAGE_OPERATIONS: AtomicUsize = AtomicUsize::new(0);
static STORAGE_TIME_TOTAL: AtomicU64 = AtomicU64::new(0);

// HIGH PRIORITY OPTIMIZATION: Hash-based program ID matching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramType {
    RaydiumLaunchpad,
    AxiomPumpSwap,
    AxiomPumpFun,
    RaydiumCpmm,
}

// Create a static HashMap for O(1) program ID lookups
static PROGRAM_ID_MAP: Lazy<HashMap<[u8; 32], ProgramType>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(*RAYDIUM_LAUNCHPAD_PROGRAM_ID_BYTES, ProgramType::RaydiumLaunchpad);
    map.insert(*AXIOM_PUMP_SWAP_PROGRAM_ID_BYTES, ProgramType::AxiomPumpSwap);
    map.insert(*AXIOM_PUMP_FUN_PROGRAM_ID_BYTES, ProgramType::AxiomPumpFun);
    map.insert(*RAYDIUM_CPMM_PROGRAM_ID_BYTES, ProgramType::RaydiumCpmm);
    map
});

// HIGH PRIORITY OPTIMIZATION: Fast program ID lookup function
#[inline]
pub fn get_program_type(account_inst_bytes: &[u8]) -> Option<ProgramType> {
    if account_inst_bytes.len() == 32 {
        let mut key = [0u8; 32];
        key.copy_from_slice(account_inst_bytes);
        PROGRAM_ID_MAP.get(&key).copied()
    } else {
        None
    }
}

pub fn get_storage_stats() -> (usize, u64) {
    (
        STORAGE_OPERATIONS.load(Ordering::Relaxed),
        STORAGE_TIME_TOTAL.load(Ordering::Relaxed),
    )
}

pub fn get_worker_stats() -> (usize, usize, usize, usize) {
    (
        WORKER_MESSAGES_RECEIVED.load(Ordering::Relaxed),
        WORKER_TRANSACTIONS_BUILT.load(Ordering::Relaxed),
        WORKER_TRANSACTIONS_INSERTED.load(Ordering::Relaxed),
        WORKER_ERRORS.load(Ordering::Relaxed),
    )
}

#[derive(Debug, Clone)]
pub struct ParsedArpcTrade {
    pub sig_bytes: Option<Arc<Vec<u8>>>,
    pub slot: u64,
    pub detection_time: Instant,
    pub tx_instructions: Arc<Vec<CompiledInstruction>>,
    pub account_keys: Arc<Vec<Vec<u8>>>,
    // Add more fields if needed for the worker
}

#[derive(Debug, Clone)]

pub struct TxWithPubkey {
    pub vendor_transactions: Vec<(String, Transaction)>, // Store all vendor transactions
    pub bonding_curve: Pubkey,
    pub mint: Pubkey,
    pub token_amount: u64,
    pub tx_type: String,
    // Use references to reduce memory footprint
    pub ray_launch_accounts: Option<RayLaunchAccounts>,
    pub ray_cpmm_accounts: Option<RaydiumCpmmPoolState>,
    pub pump_swap_accounts: Option<PumpAmmAccounts>,
    pub pump_fun_accounts: Option<PumpFunAccounts>,
    pub raydium_cpmm_accounts: Option<RayCpmmSwapAccounts>,
    pub ray_cpmm_pool_state: Option<Pubkey>,
    pub send_sig: String,
    pub send_time: Instant,
    pub send_slot: u64,
    pub created_at: Instant, // Track when this entry was created
}

impl TxWithPubkey {
    pub fn default() -> Self {
        TxWithPubkey {
            vendor_transactions: Vec::new(),
            bonding_curve: Pubkey::default(),
            mint: Pubkey::default(),
            token_amount: 0,
            tx_type: String::new(),
            ray_launch_accounts: None,
            ray_cpmm_accounts: None,
            pump_swap_accounts: None,
            pump_fun_accounts: None,
            raydium_cpmm_accounts: None,
            ray_cpmm_pool_state: None,
            send_sig: String::new(),
            send_time: Instant::now(),
            send_slot: 0,
            created_at: Instant::now(),
        }
    }
    
    /// Get transaction for a specific vendor
    pub fn get_vendor_transaction(&self, vendor_name: &str) -> Option<&Transaction> {
        self.vendor_transactions
            .iter()
            .find(|(name, _)| name == vendor_name)
            .map(|(_, tx)| tx)
    }
    
    /// Get all vendor names
    pub fn get_vendor_names(&self) -> Vec<&String> {
        self.vendor_transactions.iter().map(|(name, _)| name).collect()
    }
    
    /// Get the first available transaction (for backward compatibility)
    pub fn get_first_transaction(&self) -> Option<&Transaction> {
        self.vendor_transactions.first().map(|(_, tx)| tx)
    }
    
    /// Check if a specific vendor transaction exists
    pub fn has_vendor(&self, vendor_name: &str) -> bool {
        self.vendor_transactions.iter().any(|(name, _)| name == vendor_name)
    }
}

pub fn default_tx_with_pubkey() -> TxWithPubkey {
    TxWithPubkey::default()
}

// Global map: signature (String) -> Transaction
pub static GLOBAL_TX_MAP: Lazy<DashMap<Vec<u8>, TxWithPubkey>> = Lazy::new(DashMap::new);

static ARPC_PARSED_SENDER: OnceCell<Sender<ParsedArpcTrade>> = OnceCell::new();

/// Purge entries older than 10 seconds from GLOBAL_TX_MAP
fn purge_old_entries_task() {
    use std::time::Duration;
    
    loop {
        std::thread::sleep(Duration::from_secs(5)); // Check every 5 seconds
        
        let now = Instant::now();
        let purge_threshold = Duration::from_secs(8); // 8 seconds (was 10)
        
        let mut to_remove = Vec::new();
        let mut removed_count = 0;
        
        // Collect keys of entries to remove
        for entry in GLOBAL_TX_MAP.iter() {
            if now.duration_since(entry.value().created_at) > purge_threshold {
                to_remove.push(entry.key().clone());
            }
        }
        
        // Remove old entries
        for key in to_remove {
            if let Some((_, tx_with_pubkey)) = GLOBAL_TX_MAP.remove(&key) {
                removed_count += 1;
                println!("[Purge] Removed old entry for tx_type: {} (age: {:.2?})", 
                    tx_with_pubkey.tx_type, 
                    now.duration_since(tx_with_pubkey.created_at)
                );
            }
        }
        
        // Log map size periodically or if large cleanup occurred
        if GLOBAL_TX_MAP.len() > 0 || removed_count > 0 {
            println!("[Purge] GLOBAL_TX_MAP size: {} (removed {} entries)", GLOBAL_TX_MAP.len(), removed_count);
        }
        
        // Emergency cleanup if map is too large
        if GLOBAL_TX_MAP.len() > 1000 {
            let now = chrono::Utc::now();
            println!("[{}] - [PURGE] EMERGENCY: Map too large ({}), clearing all entries", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), 
                GLOBAL_TX_MAP.len()
            );
            GLOBAL_TX_MAP.clear();
        }
    }
}

pub fn setup_arpc_crossbeam_worker() {
    let (tx, rx) = unbounded::<ParsedArpcTrade>();
    ARPC_PARSED_SENDER.set(tx).unwrap();
    
    // Start the purging task in a separate thread
    std::thread::spawn(move || {
        purge_old_entries_task();
    });
    
    // Spawn 3 worker threads for heavy processing
    for worker_id in 0..3 {
        let rx_clone = rx.clone();
        std::thread::spawn(move || {
            // Pin worker threads to cores 5-7 for optimal performance
            if let Some(cores) = core_affinity::get_core_ids() {
                if cores.len() > 5 + worker_id {
                    core_affinity::set_for_current(cores[5 + worker_id]);
                    println!("[arpc worker {}] Pinned to core {}", worker_id, 5 + worker_id);
                }
            }
        
        // Set critical real-time priority for processing (highest priority)
        if let Err(e) = set_realtime_priority(RealtimePriority::Critical) {
            eprintln!("[arpc worker {}] Failed to set real-time priority: {}", worker_id, e);
        }
        //initial static parameter loads
        let config = GLOBAL_CONFIG.get().expect("Config not initialized");
        let buy_sol_lamports = (config.buy_sol * 1_000_000_000.0) as u64;

        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: usize = 10;

        while let Ok(parsed) = rx_clone.recv() {
            WORKER_MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);
            
            let worker_total_start = Instant::now();
            let sig_str = parsed.sig_bytes
                .as_ref()
                .map(|s| bs58::encode(s.as_slice()).into_string())
                .unwrap_or_else(|| "<no_sig>".to_string());
            
            let now = Utc::now();
            #[cfg(feature = "verbose_logging")]
            println!("[{}] - [WORKER-{}] Processing message for sig: {} (total received: {})", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), 
                worker_id,
                sig_str, 
                WORKER_MESSAGES_RECEIVED.load(Ordering::Relaxed));
            
            let mut send_tx = false;
            let mut buy_instruction = default_instruction();
            let mut mint = Pubkey::default();
            let mut ray_launch_accounts = RayLaunchAccounts::default();
            let mut pump_swap_accounts = PumpAmmAccounts::default();
            let mut pump_fun_accounts = PumpFunAccounts::default();
            let mut tx_with_pubkey: Option<TxWithPubkey> = None;
            let mut target_token_buy = 0;
            let mut raydium_cpmm_accounts = RayCpmmSwapAccounts::default();

            let parse_start = Instant::now();
            // --- OPTIMIZED INSTRUCTION MATCHING ---
            let mut instruction_count = 0;
            for instr in parsed.tx_instructions.iter() {
                instruction_count += 1;
                let instr_start = Instant::now();
                
                let program_id_index = instr.program_id_index as usize;
                let data = &instr.data;

                let account_lookup_start = Instant::now();
                if let Some(account_inst_bytes) = parsed.account_keys.get(program_id_index) {
                    let account_lookup_time = account_lookup_start.elapsed();
                    #[cfg(feature = "verbose_logging")]
                    if instruction_count <= 3 { // Only log first few instructions to avoid spam
                        println!("[PROFILE][{}] Instruction {} - Account lookup: {:.2?}", sig_str, instruction_count, account_lookup_time);
                    }
                    
                    // HIGH PRIORITY OPTIMIZATION: Hash-based program ID matching
                    let program_check_start = Instant::now();
                    if let Some(program_type) = get_program_type(account_inst_bytes) {
                        let program_check_time = program_check_start.elapsed();
                        #[cfg(feature = "verbose_logging")]
                        println!("[PROFILE][{}] Instruction {} - Program ID check: {:.2?}", sig_str, instruction_count, program_check_time);
                        
                        // Process based on program type with early exit
                        match program_type {
                            ProgramType::RaydiumLaunchpad => {
                                // Check discriminator for Raydium
                                if data.len() > 8 && &data[0..8] == [250, 234, 13, 123, 213, 156, 19, 236] {
                                    let raydium_start = Instant::now();
                                    (buy_instruction, mint, target_token_buy, ray_launch_accounts) = raydium_launchpad_build_buy_tx(
                                        &parsed.account_keys,
                                        &instr.accounts,
                                        parsed.sig_bytes.clone(),
                                        parsed.detection_time,
                                        data,
                                        buy_sol_lamports,
                                        config.buy_slippage_bps,
                                    );
                                    let raydium_time = raydium_start.elapsed();
                                    #[cfg(feature = "verbose_logging")]
                                    println!("[PROFILE][{}] Raydium launchpad processing: {:.2?}", sig_str, raydium_time);
                                    send_tx = true;
                                    let mut tx = TxWithPubkey::default();
                                    tx.tx_type = "ray_launch".to_string();
                                    tx.ray_launch_accounts = Some(ray_launch_accounts.clone());
                                    tx_with_pubkey = Some(tx);
                                    break; // Early exit after match
                                }
                            },
                            ProgramType::AxiomPumpSwap => {
                                let axiom_swap_start = Instant::now();
                                (buy_instruction, mint, target_token_buy, pump_swap_accounts) = axiom_pump_swap_build_buy_tx(
                                    &parsed.account_keys,
                                    &instr.accounts,
                                    parsed.sig_bytes.clone(),
                                    parsed.detection_time,
                                    buy_sol_lamports,
                                    config.buy_slippage_bps,
                                );
                                let axiom_swap_time = axiom_swap_start.elapsed();
                                #[cfg(feature = "verbose_logging")]
                                println!("[PROFILE][{}] Axiom pump swap processing: {:.2?}", sig_str, axiom_swap_time);
                                send_tx = true;
                                let mut tx = TxWithPubkey::default();
                                tx.tx_type = "pump_swap".to_string();
                                tx.pump_swap_accounts = Some(pump_swap_accounts.clone());
                                tx_with_pubkey = Some(tx);
                                break; // Early exit after match
                            },
                            ProgramType::AxiomPumpFun => {
                                let axiom_fun_start = Instant::now();
                                (buy_instruction, mint, target_token_buy, pump_fun_accounts) = axiom_pump_fun_build_buy_tx(
                                    &parsed.account_keys,
                                    &instr.accounts,
                                    parsed.sig_bytes.clone(),
                                    parsed.detection_time,
                                    buy_sol_lamports,
                                    config.buy_slippage_bps,
                                );
                                let axiom_fun_time = axiom_fun_start.elapsed();
                                #[cfg(feature = "verbose_logging")]
                                println!("[PROFILE][{}] Axiom pump fun processing: {:.2?}", sig_str, axiom_fun_time);
                                send_tx = true;
                                let mut tx = TxWithPubkey::default();
                                tx.tx_type = "pumpfun".to_string();
                                tx.pump_fun_accounts = Some(pump_fun_accounts.clone());
                                tx_with_pubkey = Some(tx);
                                break; // Early exit after match
                            },
                            ProgramType::RaydiumCpmm => {
                                let raydium_cpmm_start = Instant::now();
                                (buy_instruction, mint, target_token_buy, raydium_cpmm_accounts) = raydium_cpmm_build_buy_tx(
                                    &parsed.account_keys,
                                    &instr.accounts,
                                    parsed.sig_bytes.clone(),
                                    parsed.detection_time,
                                    buy_sol_lamports,
                                    config.buy_slippage_bps,
                                );
                                let raydium_cpmm_time = raydium_cpmm_start.elapsed();
                                #[cfg(feature = "verbose_logging")]
                                println!("[PROFILE][{}] Raydium CPMM processing: {:.2?}", sig_str, raydium_cpmm_time);
                                if mint != Pubkey::default() { //buy tx
                                    send_tx = true;
                                    let mut tx = TxWithPubkey::default();
                                    tx.tx_type = "ray_cpmm".to_string();
                                    tx.raydium_cpmm_accounts = Some(raydium_cpmm_accounts.clone());
                                    tx_with_pubkey = Some(tx);
                                    break; // Early exit after match
                                }
                            },
                        }
                    }
                    #[cfg(feature = "verbose_logging")]
                    {
                        let program_check_time = program_check_start.elapsed();
                        if instruction_count <= 3 {
                            println!("[PROFILE][{}] Instruction {} - Program ID check (no match): {:.2?}", sig_str, instruction_count, program_check_time);
                        }
                    }
                } else {
                    #[cfg(feature = "verbose_logging")]
                    {
                        let account_lookup_time = account_lookup_start.elapsed();
                        if instruction_count <= 3 {
                            println!("[PROFILE][{}] Instruction {} - Account lookup (failed): {:.2?}", sig_str, instruction_count, account_lookup_time);
                        }
                    }
                }
                
                #[cfg(feature = "verbose_logging")]
                {
                    let instr_time = instr_start.elapsed();
                    if instruction_count <= 3 {
                        println!("[PROFILE][{}] Instruction {} - Total processing: {:.2?}", sig_str, instruction_count, instr_time);
                    }
                }
            }
            
            let match_done = parse_start.elapsed();
            #[cfg(feature = "verbose_logging")]
            println!("[PROFILE][{}] Instruction matching ({}) total: {:.2?}", sig_str, instruction_count, match_done);
            
            if send_tx {
                WORKER_TRANSACTIONS_BUILT.fetch_add(1, Ordering::Relaxed);
                
                let build_start = Instant::now();
                let mut tx_with_pubkey = tx_with_pubkey.take().unwrap();
                
                // Build vendor-specific transactions in parallel
                let vendor_build_start = Instant::now();
                match crate::build_tx::tx_builder::build_vendor_specific_transactions_parallel(
                    buy_instruction,
                    mint,
                    target_token_buy,
                    &sig_str,
                ) {
                    Ok(vendor_transactions) => {
                        let vendor_build_time = vendor_build_start.elapsed();
                        #[cfg(feature = "verbose_logging")]
                        println!("[PROFILE][{}] Vendor transaction building: {:.2?}", sig_str, vendor_build_time);
                        
                        if !vendor_transactions.is_empty() {
                            // Store all vendor transactions
                            tx_with_pubkey.vendor_transactions = vendor_transactions.clone();
                            tx_with_pubkey.mint = mint;
                            tx_with_pubkey.token_amount = target_token_buy;
                            tx_with_pubkey.created_at = Instant::now(); // Set creation time when inserting

                            let insert_start = Instant::now();
                            
                            // Create multiple entries in GLOBAL_TX_MAP - one for each vendor transaction signature
                            // This ensures that when any vendor transaction is detected by GRPC, we can find the corresponding entry
                            let mut total_inserted = 0;
                            
                            // First, insert with the original detected signature (for backward compatibility)
                            let original_key = parsed.sig_bytes.as_ref().unwrap().as_slice().to_vec();
                            GLOBAL_TX_MAP.insert(original_key, tx_with_pubkey.clone());
                            total_inserted += 1;
                            
                            // Then, insert with each vendor transaction signature
                            for (vendor_name, transaction) in &vendor_transactions {
                                // Extract signature from the transaction
                                if let Some(signature) = transaction.signatures.first() {
                                    let vendor_sig_bytes = signature.as_ref().to_vec();
                                    
                                    // Create a copy of tx_with_pubkey for this vendor signature
                                    let mut vendor_tx_with_pubkey = tx_with_pubkey.clone();
                                    vendor_tx_with_pubkey.send_sig = signature.to_string();
                                    
                                    GLOBAL_TX_MAP.insert(vendor_sig_bytes, vendor_tx_with_pubkey);
                                    total_inserted += 1;
                                    
                                    #[cfg(feature = "verbose_logging")]
                                    println!("[PROFILE][{}] Inserted {} vendor signature: {}", 
                                        sig_str, vendor_name, signature);
                                }
                            }
                            
                            let insert_time = insert_start.elapsed();
                            
                            // Track storage performance
                            STORAGE_OPERATIONS.fetch_add(total_inserted, Ordering::Relaxed);
                            STORAGE_TIME_TOTAL.fetch_add(insert_time.as_micros() as u64, Ordering::Relaxed);
                            
                            #[cfg(feature = "verbose_logging")]
                            println!("[PROFILE][{}] Map insert time: {:.2?} (inserted {} entries)", sig_str, insert_time, total_inserted);
                            
                            WORKER_TRANSACTIONS_INSERTED.fetch_add(total_inserted, Ordering::Relaxed);

                            #[cfg(feature = "verbose_logging")]
                            {
                                let vendor_names: Vec<&String> = vendor_transactions.iter().map(|(v, _)| v).collect();
                                let now = Utc::now();
                                println!("[{}] - [WORKER] SUCCESS - {} vendor TXs built and {} entries inserted | detected for slot {} | time to parse: {:.2?} | total built: {}, total inserted: {}", 
                                    now.format("%Y-%m-%d %H:%M:%S%.3f"), 
                                    vendor_transactions.len(),
                                    total_inserted,
                                    parsed.slot, 
                                    match_done,
                                    WORKER_TRANSACTIONS_BUILT.load(Ordering::Relaxed),
                                    WORKER_TRANSACTIONS_INSERTED.load(Ordering::Relaxed));
                                
                                println!("[{}] - [WORKER] INFO - Vendor versions: {:?}", 
                                    now.format("%Y-%m-%d %H:%M:%S%.3f"),
                                    vendor_names
                                );
                            }
                            
                            // TODO: Send all vendor transactions to their respective vendors in parallel
                            // Each vendor transaction can now be sent to its corresponding vendor
                        } else {
                            WORKER_ERRORS.fetch_add(1, Ordering::Relaxed);
                            #[cfg(feature = "verbose_logging")]
                            {
                                let now = Utc::now();
                                eprintln!("[{}] - [WORKER] ERROR - No vendor transactions built for sig: {} | total errors: {}", 
                                    now.format("%Y-%m-%d %H:%M:%S%.3f"), 
                                    sig_str,
                                    WORKER_ERRORS.load(Ordering::Relaxed));
                            }
                        }
                    }
                    Err(e) => {
                        WORKER_ERRORS.fetch_add(1, Ordering::Relaxed);
                        #[cfg(feature = "verbose_logging")]
                        {
                            let now = Utc::now();
                            eprintln!("[{}] - [WORKER] ERROR - Failed to build vendor transactions for sig: {} | Error: {} | total errors: {}", 
                                now.format("%Y-%m-%d %H:%M:%S%.3f"), 
                                sig_str,
                                e,
                                WORKER_ERRORS.load(Ordering::Relaxed));
                        }
                    }
                }
            } else {
                #[cfg(feature = "verbose_logging")]
                {
                    let now = Utc::now();
                    println!("[{}] - [WORKER] No transaction to build for sig: {}", 
                        now.format("%Y-%m-%d %H:%M:%S%.3f"), sig_str);
                }
            }
            let loop_total = worker_total_start.elapsed();
            #[cfg(feature = "verbose_logging")]
            println!("[BENCH][sig={}] Total loop time: {:.2?}", sig_str, loop_total);
        }
    });
    }
}

pub fn send_parsed_arpc_trade(parsed: ParsedArpcTrade) {
    if let Some(sender) = ARPC_PARSED_SENDER.get() {
        let _ = sender.send(parsed);
    }
}

/// Manually trigger purging of old entries (useful for testing or manual cleanup)
pub fn manual_purge_old_entries() {
    let now = Instant::now();
    let purge_threshold = std::time::Duration::from_secs(10);
    
    // Use a more efficient approach - collect keys without cloning
    let mut to_remove = Vec::new();
    let mut type_counts = std::collections::HashMap::new();
    
    for entry in GLOBAL_TX_MAP.iter() {
        if now.duration_since(entry.value().created_at) > purge_threshold {
            // Store the key reference instead of cloning
            to_remove.push(entry.key().clone());
            
            // Count by transaction type for reporting
            let tx_type = entry.value().tx_type.clone();
            *type_counts.entry(tx_type).or_insert(0) += 1;
        }
    }
    
    let removed_count = to_remove.len();
    // Batch remove to reduce lock contention
    for key in to_remove {
        GLOBAL_TX_MAP.remove(&key);
    }
    
    println!("[Manual Purge] Removed {} old entries (by type: {:?}). Current map size: {}", 
        removed_count, type_counts, GLOBAL_TX_MAP.len());
}

/// Get current map size and statistics
pub fn get_map_stats() -> (usize, Vec<(String, std::time::Duration)>) {
    let now = Instant::now();
    let mut stats = Vec::new();
    
    for entry in GLOBAL_TX_MAP.iter() {
        let age = now.duration_since(entry.value().created_at);
        stats.push((entry.value().tx_type.clone(), age));
    }
    
    (GLOBAL_TX_MAP.len(), stats)
}

/// Get detailed statistics about multiple entries for the same transaction
pub fn get_multiple_entries_stats() -> (usize, std::collections::HashMap<String, usize>) {
    let mut signature_counts = std::collections::HashMap::new();
    
    for entry in GLOBAL_TX_MAP.iter() {
        let send_sig = entry.value().send_sig.clone();
        if !send_sig.is_empty() {
            *signature_counts.entry(send_sig).or_insert(0) += 1;
        }
    }
    
    let total_unique_signatures = signature_counts.len();
    (total_unique_signatures, signature_counts)
}

/// Comprehensive cleanup and debugging function
pub fn debug_and_cleanup() {
    let now = Instant::now();
    let purge_threshold = std::time::Duration::from_secs(10);
    
    let mut to_remove = Vec::new();
    let mut type_counts = std::collections::HashMap::new();
    let mut age_stats = Vec::new();
    let mut signature_counts = std::collections::HashMap::new();
    
    for entry in GLOBAL_TX_MAP.iter() {
        let age = now.duration_since(entry.value().created_at);
        let tx_type = entry.value().tx_type.clone();
        let send_sig = entry.value().send_sig.clone();
        
        // Count by type
        *type_counts.entry(tx_type.clone()).or_insert(0) += 1;
        
        // Count by signature (to see if we have multiple entries for same transaction)
        if !send_sig.is_empty() {
            *signature_counts.entry(send_sig).or_insert(0) += 1;
        }
        
        // Collect age statistics
        age_stats.push(age);
        
        // Mark for removal if old
        if age > purge_threshold {
            to_remove.push(entry.key().clone());
        }
    }
    
    // Remove old entries
    let removed_count = to_remove.len();
    for key in to_remove {
        GLOBAL_TX_MAP.remove(&key);
    }
    
    // Calculate statistics
    let total_entries = GLOBAL_TX_MAP.len();
    let avg_age = if !age_stats.is_empty() {
        age_stats.iter().sum::<std::time::Duration>() / age_stats.len() as u32
    } else {
        std::time::Duration::from_secs(0)
    };
    let max_age = age_stats.iter().max().copied().unwrap_or(std::time::Duration::from_secs(0));
    
    println!("[{}] ========== DEBUG & CLEANUP REPORT ==========", 
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    println!("[DEBUG] Map size: {} (removed {} old entries)", total_entries, removed_count);
    println!("[DEBUG] Average age: {:.2?}, Max age: {:.2?}", avg_age, max_age);
    println!("[DEBUG] Entries by type: {:?}", type_counts);
    
    // Show signature distribution (to identify multiple entries for same transaction)
    let multi_sig_entries: Vec<_> = signature_counts.iter()
        .filter(|(_, &count)| count > 1)
        .collect();
    if !multi_sig_entries.is_empty() {
        println!("[DEBUG] Multiple entries for same signature: {:?}", multi_sig_entries);
    }
    
    // Get worker stats
    let (worker_received, worker_built, worker_inserted, worker_errors) = get_worker_stats();
    println!("[DEBUG] Worker stats: Received={}, Built={}, Inserted={}, Errors={}", 
        worker_received, worker_built, worker_inserted, worker_errors);
    
    // Check for potential issues
    if worker_inserted > total_entries + removed_count {
        println!("[DEBUG] WARNING: More transactions inserted than currently in map - potential cleanup issue!");
    }
    
    if max_age > std::time::Duration::from_secs(30) {
        println!("[DEBUG] WARNING: Very old entries detected - potential memory leak!");
    }
    
    println!("[{}] ================================================", 
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
} 