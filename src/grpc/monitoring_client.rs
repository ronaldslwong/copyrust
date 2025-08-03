use crate::arpc::{
    arpc_service_client::ArpcServiceClient, SubscribeRequest as ArpcSubscribeRequest,
    SubscribeRequestFilterTransactions,
};
use crate::config_load::Config;
use crate::constants::raydium_launchpad::RAYDIUM_LAUNCHPAD_PROGRAM_ID;
use crate::constants::pump_fun::PUMP_FUN_PROGRAM_ID;
use crate::constants::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID;
use crate::build_tx::ray_cpmm::{RayCpmmSwapAccounts, get_instruction_accounts_migrate};
use std::collections::HashMap;
use std::sync::Arc;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio::time::{sleep, Duration};
use chrono::Utc;
use core_affinity;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::build_tx::pump_swap::{get_instruction_accounts_migrate_pump, PumpAmmAccounts};
use std::time::Instant;

/// Global struct to store monitoring data
#[derive(Debug, Clone)]
pub struct MonitoringData {
    pub mint_pubkey: Pubkey,
    pub timestamp: u64,
    pub ray_cpmm_accounts: RayCpmmSwapAccounts,
    pub pump_fun_accounts: PumpAmmAccounts,
}

/// Global storage for monitoring data
pub static GLOBAL_MONITORING_DATA: Lazy<DashMap<Pubkey, MonitoringData>> = Lazy::new(|| {
    DashMap::new()
});



// Global monitoring statistics
static MONITORING_MESSAGES_RECEIVED: AtomicUsize = AtomicUsize::new(0);
static MONITORING_TRANSACTIONS_LOGGED: AtomicUsize = AtomicUsize::new(0);
static MONITORING_ERRORS: AtomicUsize = AtomicUsize::new(0);

pub fn get_monitoring_stats() -> (usize, usize, usize) {
    (
        MONITORING_MESSAGES_RECEIVED.load(Ordering::Relaxed),
        MONITORING_TRANSACTIONS_LOGGED.load(Ordering::Relaxed),
        MONITORING_ERRORS.load(Ordering::Relaxed),
    )
}

// REMOVED: DexActivityLog struct and GLOBAL_DEX_LOGS storage
// This was causing significant performance overhead due to:
// - Expensive map insertions on every transaction
// - Frequent purging operations (O(n) iteration)
// - Memory allocation for large data structures
// - Concurrent access overhead from DashMap

// REMOVED: purge_old_dex_logs function
// This was causing significant performance overhead due to:
// - O(n) iteration through all entries every 30 seconds
// - Memory allocation for removal lists
// - Lock contention during removal operations

/// Purge old monitoring data (older than 30 seconds)
fn purge_old_monitoring_data() {
    loop {
        // OPTIMIZATION: More frequent cleanup (was 30 seconds)
        std::thread::sleep(Duration::from_secs(10)); // Was 30 seconds
        
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut to_remove = Vec::new();
        
        // OPTIMIZATION: Reduced retention from 60 to 30 seconds
        for entry in GLOBAL_MONITORING_DATA.iter() {
            if current_time - entry.value().timestamp > 30 { // Was 60 seconds
                to_remove.push(*entry.key());
            }
        }
        
        // Remove old entries
        for key in to_remove {
            GLOBAL_MONITORING_DATA.remove(&key);
        }
        
        // OPTIMIZATION: Emergency cleanup if map gets too large
        if GLOBAL_MONITORING_DATA.len() > 300 { // Reduced from 500 to 300
            println!("[Monitoring] WARNING: Monitoring data map too large ({} entries), clearing...", GLOBAL_MONITORING_DATA.len());
            GLOBAL_MONITORING_DATA.clear();
        }
        
        // OPTIMIZATION: Log cleanup stats periodically
        if GLOBAL_MONITORING_DATA.len() > 50 {
            println!("[Monitoring] Data cleanup: {} entries remaining", GLOBAL_MONITORING_DATA.len());
        }
    }
}

/// Emergency cleanup function for memory leak situations
pub fn emergency_cleanup_monitoring_data() {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut to_remove = Vec::new();
    
    // Remove data older than 30 seconds (more aggressive than normal 60 seconds)
    for entry in GLOBAL_MONITORING_DATA.iter() {
        if current_time - entry.value().timestamp > 30 { // 30 seconds
            to_remove.push(entry.key().clone());
        }
    }
    
    // Remove old entries
    let removed_count = to_remove.len();
    for key in to_remove {
        GLOBAL_MONITORING_DATA.remove(&key);
    }
    
    if removed_count > 0 {
        let now = Utc::now();
        println!("[{}] - [MONITORING] Emergency cleanup: removed {} entries, remaining: {}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), 
            removed_count, 
            GLOBAL_MONITORING_DATA.len()
        );
    }
    
    // If still too large, clear everything
    if GLOBAL_MONITORING_DATA.len() > 200 {
        let now = Utc::now();
        println!("[{}] - [MONITORING] EMERGENCY: Still too large ({}), clearing all entries", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), 
            GLOBAL_MONITORING_DATA.len()
        );
        GLOBAL_MONITORING_DATA.clear();
    }
}

/// Start ARPC monitoring subscription (separate from trading ARPC)
pub async fn start_arpc_monitoring_subscription(
    endpoint: &str,
    programs_to_monitor: Vec<String>,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = Utc::now();
    println!("[{}] - [Monitoring ARPC] Attempting to connect to: {}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), endpoint);
    let mut client = ArpcServiceClient::connect(endpoint.to_string()).await?;
    let now = Utc::now();
    println!("[{}] - [Monitoring ARPC] Connection successful!", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    let mut filters: HashMap<String, SubscribeRequestFilterTransactions> = HashMap::new();
    
    if !programs_to_monitor.is_empty() {
        filters.insert(
            "transactions".to_string(),
            SubscribeRequestFilterTransactions {
                account_include: programs_to_monitor.clone(),
                account_exclude: vec![],
                account_required: vec![],
            },
        );
    }

    let (tx, rx) = mpsc::channel(256); // Larger buffer for monitoring
    let request_stream = ReceiverStream::new(rx);

    // Send the initial subscription request
    let initial_request = ArpcSubscribeRequest {
        transactions: filters,
        ping_id: None,
    };
    let now = Utc::now();
    println!("[{}] - [Monitoring ARPC] Sending subscription request: {:?}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), initial_request);
    tx.send(initial_request).await?;

    let mut stream = client.subscribe(request_stream).await?.into_inner();

    let now = Utc::now();
    println!("[{}] - [Monitoring ARPC] DEX activity subscription established. Monitoring {} programs...", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), programs_to_monitor.len());
    let now = Utc::now();
    println!("[{}] - [Monitoring ARPC] Programs to monitor: {:?}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), programs_to_monitor);

    // Start the purging task in a separate thread
    std::thread::spawn(move || {
        purge_old_monitoring_data();
    });

    // Start a periodic stats reporting task
    let _stats_config = config.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60)); // Report every minute
        loop {
            interval.tick().await;
            let (received, logged, errors) = get_monitoring_stats();
            let now = Utc::now();
            println!("[{}] - [MONITORING ARPC STATS] Received: {}, Logged: {}, Errors: {}, Processing Rate: {:.2}%", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"),
                received, logged, errors,
                if received > 0 { (logged as f64 / received as f64) * 100.0 } else { 0.0 }
            );
            
            // REMOVED: DEX logs stats (performance optimization)
            
            // Log monitoring data stats
            println!("[{}] - [MONITORING ARPC STATS] GLOBAL_MONITORING_DATA size: {}", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"),
                GLOBAL_MONITORING_DATA.len()
            );
        }
    });

    // Pin the monitoring thread to core 15 (last core, separate from trading cores 0-3)
    // This ensures monitoring doesn't interfere with critical trading operations
    if let Some(cores) = core_affinity::get_core_ids() {
        if cores.len() > 15 {
            core_affinity::set_for_current(cores[15]);
            let now = Utc::now();
            println!("[{}] - [Monitoring ARPC] Thread pinned to core 15 (last core)", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"));
        } else if cores.len() > 4 {
            // Fallback to core 4 if we don't have 16 cores
            core_affinity::set_for_current(cores[4]);
            let now = Utc::now();
            println!("[{}] - [Monitoring ARPC] Thread pinned to core 4 (fallback)", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"));
        }
    }
    
    // Set lower priority for monitoring (shouldn't interfere with trading)
    if let Err(e) = crate::utils::rt_scheduler::set_realtime_priority(crate::utils::rt_scheduler::RealtimePriority::Low) {
        let now = Utc::now();
        eprintln!("[{}] - [Monitoring ARPC] Failed to set real-time priority: {}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), e);
    }

    while let Some(result) = stream.message().await? {
        let result = result.clone();
        
        // Increment received counter
        MONITORING_MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);
        
        let sig_str = result.transaction.as_ref()
            .and_then(|tx| tx.signatures.get(0))
            .map(|sig| bs58::encode(sig).into_string())
            .unwrap_or_else(|| "<no_sig>".to_string());
        
        // println!("[Monitoring ARPC] Received message: {}", sig_str);
        
        // Process in a separate task to avoid blocking
        tokio::spawn(async move {
            let processing_start = std::time::Instant::now();
            
                    match process_monitoring_message(&result, "arpc").await {
            Ok(_) => {
                MONITORING_TRANSACTIONS_LOGGED.fetch_add(1, Ordering::Relaxed);
                let processing_time = processing_start.elapsed();
                let now = Utc::now();
                // println!("[{}] - [MONITORING ARPC] Processed message (processing time: {:.2?})", 
                //     now.format("%Y-%m-%d %H:%M:%S%.3f"), processing_time);
            }
            Err(e) => {
                MONITORING_ERRORS.fetch_add(1, Ordering::Relaxed);
                let processing_time = processing_start.elapsed();
                let now = Utc::now();
                #[cfg(feature = "verbose_logging")]
                eprintln!("[{}] - [MONITORING ARPC] Error processing message: {} (processing time: {:.2?})", 
                    now.format("%Y-%m-%d %H:%M:%S%.3f"), e, processing_time);
            }
        }
        });
    }

    Ok(())
}



async fn process_monitoring_message(
    result: &crate::arpc::SubscribeResponse,
    feed_type: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let transaction = match &result.transaction {
        Some(tx) => tx,
        None => return Ok(()),
    };

    let signature = transaction.signatures.get(0)
        .map(|sig| bs58::encode(sig).into_string())
        .unwrap_or_else(|| "<no_sig>".to_string());

    let slot = transaction.slot;
    let timestamp = Utc::now();
    let detection_time = std::time::Instant::now();

    // Extract program IDs from instructions
    let mut program_ids = Vec::new();
    let mut account_keys = Vec::new();
    
    // Convert account keys to strings
    for key in &transaction.account_keys {
        account_keys.push(bs58::encode(key).into_string());
    }

    // Extract program IDs from instructions
    for instruction in &transaction.instructions {
        let program_id_index = instruction.program_id_index as usize;
        if let Some(account_key) = account_keys.get(program_id_index) {
            program_ids.push(account_key.clone());
        }
    }

    // ========================================
    // 🎯 INSERT YOUR CUSTOM PARSING LOGIC HERE
    // ========================================
    
    // Example: Parse specific program instructions
    for instruction in transaction.instructions.iter() {
        let program_id_index = instruction.program_id_index as usize;
        if let Some(program_id) = account_keys.get(program_id_index) {

            match program_id.as_str() {
                RAYDIUM_LAUNCHPAD_PROGRAM_ID => {
                    if instruction.data == [136, 92, 200, 103, 28, 218, 144, 140] { //migrate instruction
                        // Parse Raydium Launchpad instructions
                        parse_raydium_launchpad_instruction(instruction, &transaction.account_keys, &signature, slot);
                    }

                }
                PUMP_FUN_PROGRAM_ID => {
                    if instruction.data == [155, 234, 231, 146, 236, 158, 162, 30] { //migrate instruction
                        parse_pump_fun_instruction(instruction, &transaction.account_keys, &signature, slot);
                    }
                }
                _ => {
                    // Handle other programs
                }
            }
        }
    }

    // ========================================
    // END OF CUSTOM PARSING LOGIC
    // ========================================

    // REMOVED: Log creation and storage (performance optimization)
    // This was causing significant overhead due to:
    // - Expensive struct allocation
    // - Map insertion operations
    // - Memory allocation for large data structures

    Ok(())
}

// Example parsing functions (implement these based on your needs)
fn parse_raydium_launchpad_instruction(
    instruction: &crate::arpc::CompiledInstruction,
    account_keys: &[Vec<u8>],
    signature: &str,
    slot: u64,
) {
    // Parse Raydium Launchpad specific logic
    let now = Utc::now();
    println!("[{}] - [PARSER] Raydium Launchpad migration instruction detected: sig={}, slot={}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), signature, slot);
    
    let migrated_accounts = get_instruction_accounts_migrate(&account_keys, &instruction.accounts);

    // Get the mint pubkey (token_1_mint from RayCpmmSwapAccounts)
    let mint_pubkey = migrated_accounts.token_1_mint;
    
    // Create monitoring data
    let monitoring_data = MonitoringData {
        mint_pubkey,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        ray_cpmm_accounts: migrated_accounts,
        pump_fun_accounts: PumpAmmAccounts::default(),
    };
    
    // Store in global map
    GLOBAL_MONITORING_DATA.insert(mint_pubkey, monitoring_data.clone());
    
    let now = Utc::now();
    println!("[{}] - [PARSER] Stored monitoring data for mint: {}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), mint_pubkey);
    let now = Utc::now();
    println!("[{}] - [PARSER] Global monitoring data size: {}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), GLOBAL_MONITORING_DATA.len());
    
}



fn parse_pump_fun_instruction(
    instruction: &crate::arpc::CompiledInstruction,
    account_keys: &[Vec<u8>],
    signature: &str,
    slot: u64,
) {
    // Parse instruction data bytes
    let now = Utc::now();
    println!("[{}] - [PARSER] Instruction data: sig={}, slot={}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), signature, slot);
    
    // Example: Check for specific instruction discriminators
    let migrated_accounts = get_instruction_accounts_migrate_pump(&account_keys, &instruction.accounts);

    // Get the mint pubkey (token_1_mint from RayCpmmSwapAccounts)
    let mint_pubkey = migrated_accounts.base_mint;
    
    // Create monitoring data
    let monitoring_data = MonitoringData {
        mint_pubkey,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        ray_cpmm_accounts: RayCpmmSwapAccounts::default(),
        pump_fun_accounts: migrated_accounts,
    };
    
    // Store in global map
    GLOBAL_MONITORING_DATA.insert(mint_pubkey, monitoring_data.clone());
    
    let now = Utc::now();
    println!("[{}] - [PARSER] Stored monitoring data for mint: {}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), mint_pubkey);
    let now = Utc::now();
    println!("[{}] - [PARSER] Global monitoring data size: {}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), GLOBAL_MONITORING_DATA.len());
    
}


/// Start monitoring with retry (ARPC)
pub async fn start_arpc_monitoring_with_retry(
    endpoint: &str,
    programs_to_monitor: Vec<String>,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let now = Utc::now();
        println!("[{}] - [Monitoring ARPC] Attempt {} to connect and start monitoring...", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), attempt);
        let now = Utc::now();
        println!("[{}] - [Monitoring ARPC] Connecting to endpoint: {}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), endpoint);
        let result = start_arpc_monitoring_subscription(endpoint, programs_to_monitor.clone(), config.clone()).await;
        match result {
            Ok(_) => {
                let now = Utc::now();
                println!("[{}] - [Monitoring ARPC] Subscription ended gracefully.", 
                    now.format("%Y-%m-%d %H:%M:%S%.3f"));
                break;
            }
            Err(e) => {
                let now = Utc::now();
                eprintln!("[{}] - [Monitoring ARPC] Subscription error: {}", 
                    now.format("%Y-%m-%d %H:%M:%S%.3f"), e);
                let now = Utc::now();
                eprintln!("[{}] - [Monitoring ARPC] Error details: {:?}", 
                    now.format("%Y-%m-%d %H:%M:%S%.3f"), e);
                let now = Utc::now();
                eprintln!("[{}] - [Monitoring ARPC] Retrying in 10 seconds...", 
                    now.format("%Y-%m-%d %H:%M:%S%.3f"));
                sleep(Duration::from_secs(10)).await;
            }
        }
    }
    Ok(())
}



// REMOVED: DEX logs utility functions (performance optimization)
// These functions were causing significant overhead due to:
// - Expensive map iterations
// - Memory allocation for result vectors
// - String comparisons and filtering operations

/// Get monitoring data for a specific mint
pub fn get_monitoring_data(mint_pubkey: &Pubkey) -> Option<MonitoringData> {
    GLOBAL_MONITORING_DATA.get(mint_pubkey).map(|entry| entry.value().clone())
}

/// Get all monitoring data
pub fn get_all_monitoring_data() -> Vec<MonitoringData> {
    GLOBAL_MONITORING_DATA.iter()
        .map(|entry| entry.value().clone())
        .collect()
}

/// Get monitoring data count
pub fn get_monitoring_data_count() -> usize {
    GLOBAL_MONITORING_DATA.len()
}

/// Export monitoring data for external use
pub fn export_monitoring_data() -> Vec<(Pubkey, MonitoringData)> {
    GLOBAL_MONITORING_DATA.iter()
        .map(|entry| (*entry.key(), entry.value().clone()))
        .collect()
} 