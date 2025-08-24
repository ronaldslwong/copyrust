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
// REMOVED: use crate::grpc::arpc_worker::GLOBAL_TX_MAP; - ARPC is decommissioned
use crate::build_tx::pump_fun::{build_sell_instruction, get_bonding_curve_state, BondingCurve};
use crate::build_tx::pump_swap::build_pump_sell_instruction;

use crate::build_tx::ray_launch::build_ray_launch_sell_instruction;
use crate::build_tx::ray_cpmm::{build_ray_cpmm_sell_instruction};
use crate::grpc::programs::meteora_bonding::meteora_bonding_build_sell_tx;
use crate::config_load::GLOBAL_CONFIG;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::time::Instant;
use std::thread;
use std::time::Duration;
use crate::grpc::monitoring_client::GLOBAL_MONITORING_DATA;
use crate::send_tx::jito::send_jito_bundle;
use crate::send_tx::jito::create_instruction_jito;
use crate::send_tx::generic_sender::send_all_vendors_parallel;
use crate::grpc::utils;
use crate::triton_grpc::crossbeam_worker::{ParsedTx, ASYNC_RUNTIME};

// Import constants for program IDs
use crate::constants::raydium_launchpad::RAYDIUM_LAUNCHPAD_PROGRAM_ID_BYTES;
use crate::constants::axiom::{AXIOM_PUMP_SWAP_PROGRAM_ID_BYTES, AXIOM_PUMP_FUN_PROGRAM_ID_BYTES};
use crate::constants::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID_BYTES;
use crate::constants::pump_fun::PUMP_FUN_PROGRAM_ID_BYTES;
use crate::constants::pump_swap::PUMP_SWAP_PROGRAM_ID_BYTES;
use crate::constants::heaven::HEAVEN_PROGRAM_ID_BYTES;
use crate::constants::boop_fun::BOOP_FUN_PROGRAM_ID_BYTES;
// Import program building functions
use crate::grpc::programs::raydium_launchpad::raydium_launchpad_build_buy_tx;
// use crate::grpc::programs::axiom::axiom_pump_swap_build_buy_tx;
// use crate::grpc::programs::axiom::axiom_pump_fun_build_buy_tx;
use crate::grpc::programs::raydium_cpmm::raydium_cpmm_build_buy_tx;
use crate::grpc::programs::pumpfun::pump_fun_build_buy_tx;
use crate::grpc::programs::pumpswap::pump_swap_build_buy_tx;
use crate::grpc::programs::heaven::heaven_build_buy_tx;
use crate::grpc::programs::boop_fun::boop_fun_build_buy_tx;
// Program type detection (adapted from ARPC worker)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramType {
    RaydiumLaunchpad,
    // AxiomPumpSwap,
    // AxiomPumpFun,
    RaydiumCpmm,
    PumpFun,
    PumpSwap,
    Heaven,
    BoopFun,
}

// Structure to store account information for different DEX types
#[derive(Debug, Clone, Default)]
struct StoredAccounts {
    pub pump_fun_accounts: Option<crate::build_tx::pump_fun::PumpFunAccounts>,
    pub pump_swap_accounts: Option<crate::build_tx::pump_swap::PumpAmmAccounts>,
    pub ray_launch_accounts: Option<crate::build_tx::ray_launch::RayLaunchAccounts>,
    pub raydium_cpmm_accounts: Option<crate::build_tx::ray_cpmm::RayCpmmSwapAccounts>,
    pub heaven_accounts: Option<crate::build_tx::heaven::HeavenAccounts>,
    pub boop_fun_accounts: Option<crate::build_tx::boop_fun::BoopFunAccounts>,
}

// Create a static HashMap for O(1) program ID lookups
use std::collections::HashMap;
use once_cell::sync::Lazy;

static PROGRAM_ID_MAP: Lazy<HashMap<[u8; 32], ProgramType>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(*RAYDIUM_LAUNCHPAD_PROGRAM_ID_BYTES, ProgramType::RaydiumLaunchpad);
    // map.insert(*AXIOM_PUMP_SWAP_PROGRAM_ID_BYTES, ProgramType::AxiomPumpSwap);
    // map.insert(*AXIOM_PUMP_FUN_PROGRAM_ID_BYTES, ProgramType::AxiomPumpFun);
    map.insert(*RAYDIUM_CPMM_PROGRAM_ID_BYTES, ProgramType::RaydiumCpmm);
    map.insert(*PUMP_FUN_PROGRAM_ID_BYTES, ProgramType::PumpFun);
    map.insert(*PUMP_SWAP_PROGRAM_ID_BYTES, ProgramType::PumpSwap);
    map.insert(*HEAVEN_PROGRAM_ID_BYTES, ProgramType::Heaven);
    map.insert(*BOOP_FUN_PROGRAM_ID_BYTES, ProgramType::BoopFun);
    map
});

// Fast program ID lookup function
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

// NEW: Custom function to send transactions and store info for ALL signatures
async fn send_and_store_all_signatures(
    vendor_transactions: &[(String, solana_sdk::transaction::Transaction)],
    detection_time: std::time::Instant,
    tx_type: String,
    mint: solana_sdk::pubkey::Pubkey,
    target_token_buy: u64,
    parsed_sol_amount: u64,
    parsed_instructions: Option<Vec<solana_sdk::instruction::Instruction>>,
    parsed_account_keys: Option<Vec<solana_sdk::pubkey::Pubkey>>,
    parsed_feed_id: String,
    parsed_slot: u64,
    pump_fun_accounts: Option<crate::build_tx::pump_fun::PumpFunAccounts>,
    pump_swap_accounts: Option<crate::build_tx::pump_swap::PumpAmmAccounts>,
    ray_launch_accounts: Option<crate::build_tx::ray_launch::RayLaunchAccounts>,
    raydium_cpmm_accounts: Option<crate::build_tx::ray_cpmm::RayCpmmSwapAccounts>,
    heaven_accounts: Option<crate::build_tx::heaven::HeavenAccounts>,
    boop_fun_accounts: Option<crate::build_tx::boop_fun::BoopFunAccounts>,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    use chrono::Utc;
    use futures::future::join_all;
    
    println!("[{}] - [TRITON] Sending transactions to {} vendors and storing ALL signatures", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), vendor_transactions.len());
    
    // Create futures for all vendor sends
    let mut futures = Vec::new();
    for (vendor_name, transaction) in vendor_transactions {
        let vendor_name = vendor_name.clone();
        let transaction = transaction.clone();
        let future = async move {
            let result = crate::send_tx::generic_sender::send_to_vendor(&vendor_name, &transaction).await;
            (vendor_name, result)
        };
        futures.push(Box::pin(future));
    }
    
    // Execute all futures in parallel
    let results = join_all(futures).await;
    
    // Process results and store transaction info for ALL successful signatures
    let mut successful_vendors = Vec::new();
    let mut all_signatures = Vec::new();
    
    for (vendor_name, result) in results {
        
        match result {
            Ok(signature) => {
                // SPECIAL DEBUGGING FOR ZEROSLOT
                if vendor_name == "zeroslot" {
                    println!("[{}] - [TRITON] 🔍 DEBUG - Zeroslot succeeded with signature: {}", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), signature);
                    println!("[{}] - [TRITON] 🔍 DEBUG - Signature length: {} chars", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), signature.len());
                    println!("[{}] - [TRITON] 🔍 DEBUG - Attempting to decode signature to bytes...", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                }
                
                successful_vendors.push((vendor_name.clone(), signature.clone()));
                all_signatures.push(signature.clone());
                
                // Store transaction info for THIS signature with account structures
                if let Ok(signature_bytes) = bs58::decode(&signature).into_vec() {
                    if vendor_name == "zeroslot" {
                        println!("[{}] - [TRITON] 🔍 DEBUG - Zeroslot signature decoded successfully to {} bytes", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), signature_bytes.len());
                    }
                    
                    println!("[{}] - [TRITON] DEBUG - Storing transaction info: target_token_buy={}, parsed_sol_amount={}", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), target_token_buy, parsed_sol_amount);
                    
                    // NEW: Get nonce information for tracking (we'll need to pass this from the caller)
                    // For now, we'll use None since this function doesn't have access to nonce info
                    // TODO: Update this function to accept and pass through nonce information
                    crate::triton_grpc::crossbeam_worker::store_transaction_info_with_nonce(
                        signature_bytes,
                        parsed_slot,
                        tx_type.clone(),
                        mint,
                        target_token_buy,
                        parsed_sol_amount,
                        pump_fun_accounts.clone(),
                        pump_swap_accounts.clone(),
                        ray_launch_accounts.clone(),
                        raydium_cpmm_accounts.clone(),
                        heaven_accounts.clone(),
                        boop_fun_accounts.clone(),
                        detection_time,
                        parsed_feed_id.clone(),
                        parsed_instructions.clone(),
                        parsed_account_keys.clone(),
                        None, // nonce_index - TODO: Pass this from caller
                        None, // nonce_pubkey - TODO: Pass this from caller
                    );
                    
                    println!("[{}] - [TRITON] Stored transaction info for {} signature: {} (type: {}, mint: {}) with account structures", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), vendor_name, signature, tx_type, mint);
                } else {
                    // SPECIAL DEBUGGING FOR ZEROSLOT
                    if vendor_name == "zeroslot" {
                        eprintln!("[{}] - [TRITON] ❌ ERROR - Failed to decode zeroslot signature '{}' to bytes", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), signature);
                        eprintln!("[{}] - [TRITON] ❌ ERROR - Signature decode error: {:?}", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), bs58::decode(&signature).into_vec());
                    }
                }
            }
            Err(e) => {
                // SPECIAL DEBUGGING FOR ZEROSLOT
                if vendor_name == "zeroslot" {
                    eprintln!("[{}] - [TRITON] ❌ ERROR - Zeroslot failed with error: {:?}", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), e);
                }
                eprintln!("[TRITON] {} failed: {}", vendor_name, e);
            }
        }
    }
    
    // Return the fastest successful vendor (for backward compatibility)
    if let Some((fastest_vendor, fastest_signature)) = successful_vendors.first() {
        println!("[{}] - [TRITON] SUCCESS: {} vendors succeeded. {} won with sig: {}. Total signatures stored: {}", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), 
            successful_vendors.len(), 
            fastest_vendor, 
            fastest_signature, 
            all_signatures.len());
        
        Ok((fastest_vendor.clone(), fastest_signature.clone()))
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "All vendors failed to send transaction"
        )))
    }
}

pub fn build_and_send_tx(parsed: ParsedTx) {
    // PERFORMANCE TRACKING: Record when we start building the transaction
    let tx_build_start = std::time::Instant::now();
    
    // PERFORMANCE: Add detailed timing for different phases
    let mut phase_timings = std::collections::HashMap::new();
    let mut last_checkpoint = tx_build_start;
    
    //initial static parameter loads
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    let buy_sol_lamports = (config.buy_sol * 1_000_000_000.0) as u64;
    
    // PERFORMANCE: Checkpoint 1 - Initial setup
    let now = std::time::Instant::now();
    phase_timings.insert("setup", now.duration_since(last_checkpoint));
    last_checkpoint = now;

    // Extract transaction data from parsed
    let instructions = match &parsed.instructions {
        Some(instrs) => instrs.clone(),
        None => {
            eprintln!("[ERROR] No instructions found in parsed transaction");
            return;
        }
    };
    
    let account_keys = match &parsed.account_keys {
        Some(keys) => keys.clone(),
        None => {
            eprintln!("[ERROR] No account keys found in parsed transaction");
            return;
        }
    };
    
    // Get signature for logging
    let sig_string = parsed.sig_bytes.as_ref()
        .map(|s| bs58::encode(s).into_string())
        .unwrap_or_default();
    
    println!("[{}] - [TRITON] Building identical transaction for sig: {} with {} instructions", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, instructions.len());
    
    // DEBUG: Show parsed amounts from the transaction
    println!("[TRITON] DEBUG - Parsed amounts from transaction: SOL={:?} lamports, Mint={:?}",
        parsed.sol_buy_amount_lamports, parsed.mint_token_amount);
    
    // PERFORMANCE: Checkpoint 2 - Data extraction
    let now = std::time::Instant::now();
    phase_timings.insert("data_extraction", now.duration_since(last_checkpoint));
    last_checkpoint = now;
    
    // Process instructions to determine transaction type and build appropriate transactions
    let mut send_tx = false;
    let mut buy_instruction = Instruction {
        program_id: Pubkey::default(),
        accounts: vec![],
        data: vec![],
    };
    let mut mint = Pubkey::default();
    let mut target_token_buy = 0u64;
    let mut tx_type = String::new();
    let mut stored_accounts = StoredAccounts::default();
    
    // Use parsed amounts if available, otherwise fall back to config
    let sol_amount_to_use = parsed.sol_buy_amount_lamports.unwrap_or(buy_sol_lamports);
    let mint_amount_to_use = parsed.mint_token_amount.unwrap_or(0);
    // OPTIMIZATION: Reduce debug logging in critical path
    #[cfg(feature = "verbose_logging")]
    {
        println!("[TRITON] DEBUG - Using amounts: SOL={} lamports (parsed: {:?}, config: {}), Mint={} (parsed: {:?})", 
            sol_amount_to_use, parsed.sol_buy_amount_lamports, buy_sol_lamports, mint_amount_to_use, parsed.mint_token_amount);
    }
    let mut token_2022 = false;
    
    // Process each instruction to find the one we can handle
    for (instruction_count, instruction) in instructions.iter().enumerate() {
        let program_id_bytes = instruction.program_id.to_bytes();
        // HIGH PRIORITY OPTIMIZATION: Hash-based program ID matching
        if let Some(program_type) = get_program_type(&program_id_bytes) {
            // PERFORMANCE TRACKING: Record when we match program type
            let program_match_time = std::time::Instant::now();
            
            // PERFORMANCE: Checkpoint 3 - Program type detection
            let now = std::time::Instant::now();
            phase_timings.insert("program_detection", now.duration_since(last_checkpoint));
            last_checkpoint = now;
            
            // OPTIMIZATION: Reduce debug logging in critical path
            #[cfg(feature = "verbose_logging")]
            {
                println!("[{}] - [TRITON] Instruction {} - Program type detected: {:?}", 
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), instruction_count, program_type);
            }
            let data = &instruction.data;

            match program_type {
                ProgramType::PumpFun => {
                    if data.len() > 8 && &data[0..8] == [102, 6, 61, 18, 1, 218, 235, 234] {

                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[{}] - [TRITON] Processing Pump.fun instruction", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                        }
                        

                        let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                    
                        // Convert sig_bytes to Arc<Vec<u8>> format
                        let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                        
                        let (instruction_result, mint_val, target_val, pump_fun_accounts) = pump_fun_build_buy_tx(
                            &instruction.accounts,
                            sig_bytes_arc,
                            detection_time,
                            buy_sol_lamports,
                            sol_amount_to_use, // grpc_sol parameter
                            mint_amount_to_use, // grpc_token parameter
                            config.buy_slippage_bps,
                        );

                        // Set the required variables for the rest of the function
                        buy_instruction = instruction_result;
                        mint = mint_val;
                        target_token_buy = target_val;
                        tx_type = "pumpfun".to_string();
                        send_tx = true;
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - PumpFun: target_token_buy={}, mint={}", target_token_buy, mint);
                        }
                        
                        // Store the account structures for later use in sell transactions
                        stored_accounts.pump_fun_accounts = Some(pump_fun_accounts);
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - Stored pump_fun_accounts: {:?}", stored_accounts.pump_fun_accounts);
                        }
                        
                        // PERFORMANCE: Checkpoint 4 - PumpFun builder
                        let now = std::time::Instant::now();
                        phase_timings.insert("pumpfun_builder", now.duration_since(last_checkpoint));
                        last_checkpoint = now;
                        
                        break;
                    }
                                            
                }

                ProgramType::PumpSwap => {
                    if data.len() > 8 && &data[0..8] == [102, 6, 61, 18, 1, 218, 235, 234] {

                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[{}] - [TRITON] Processing Pump.swap instruction", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                        }
                        

                        let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                    
                        // Convert sig_bytes to Arc<Vec<u8>> format
                        let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                        
                        let (instruction_result, mint_val, target_val, pump_swap_accounts) = pump_swap_build_buy_tx(
                            &instruction.accounts,
                            sig_bytes_arc,
                            detection_time,
                            buy_sol_lamports,
                            sol_amount_to_use, // grpc_sol parameter
                            mint_amount_to_use, // grpc_token parameter
                            config.buy_slippage_bps,
                        );

                        // Set the required variables for the rest of the function
                        buy_instruction = instruction_result;
                        mint = mint_val;
                        target_token_buy = target_val;
                        tx_type = "pump_swap".to_string();
                        send_tx = true;
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - PumpFun: target_token_buy={}, mint={}", target_token_buy, mint);
                        }
                        
                        // Store the account structures for later use in sell transactions
                        stored_accounts.pump_swap_accounts = Some(pump_swap_accounts);
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - Stored pump_fun_accounts: {:?}", stored_accounts.pump_swap_accounts);
                        }
                        
                        // PERFORMANCE: Checkpoint 4 - PumpFun builder
                        let now = std::time::Instant::now();
                        phase_timings.insert("pumpfun_builder", now.duration_since(last_checkpoint));
                        last_checkpoint = now;
                        
                        break;
                    }
                                            
                }

                

                ProgramType::Heaven => {
                    if data.len() > 8 && &data[0..8] == [102, 6, 61, 18, 1, 218, 235, 234] {

                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[{}] - [TRITON] Processing Heaven instruction", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                        }
                        

                        let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                    
                        // Convert sig_bytes to Arc<Vec<u8>> format
                        let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                        
                        let (instruction_result, mint_val, target_val, heaven_accounts) = heaven_build_buy_tx(
                            &instruction.accounts,
                            sig_bytes_arc,
                            detection_time,
                            buy_sol_lamports,
                            sol_amount_to_use, // grpc_sol parameter
                            mint_amount_to_use, // grpc_token parameter
                            config.buy_slippage_bps,
                        );

                        // Set the required variables for the rest of the function
                        buy_instruction = instruction_result;
                        mint = mint_val;
                        target_token_buy = target_val;
                        tx_type = "heaven".to_string();
                        token_2022 = true;
                        send_tx = true;
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - PumpFun: target_token_buy={}, mint={}", target_token_buy, mint);
                        }
                        
                        // Store the account structures for later use in sell transactions
                        stored_accounts.heaven_accounts = Some(heaven_accounts);
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - Stored pump_fun_accounts: {:?}", stored_accounts.pump_swap_accounts);
                        }
                        
                        // PERFORMANCE: Checkpoint 4 - PumpFun builder
                        let now = std::time::Instant::now();
                        phase_timings.insert("heaven_builder", now.duration_since(last_checkpoint));
                        last_checkpoint = now;
                        
                        break;
                    }
                                            
                }



                ProgramType::BoopFun => {
                    if data.len() > 8 && &data[0..8] == [8, 167, 240, 229, 178, 101, 119, 54] {

                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[{}] - [TRITON] Processing Heaven instruction", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                        }
                        

                        let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                    
                        // Convert sig_bytes to Arc<Vec<u8>> format
                        let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                        
                        let (instruction_result, mint_val, target_val, boop_fun_accounts) = boop_fun_build_buy_tx(
                            &instruction.accounts,
                            sig_bytes_arc,
                            detection_time,
                            buy_sol_lamports,
                            sol_amount_to_use, // grpc_sol parameter
                            mint_amount_to_use, // grpc_token parameter
                            config.buy_slippage_bps,
                        );

                        // Set the required variables for the rest of the function
                        buy_instruction = instruction_result;
                        mint = mint_val;
                        target_token_buy = target_val;
                        tx_type = "boop_fun".to_string();
                        send_tx = true;
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - PumpFun: target_token_buy={}, mint={}", target_token_buy, mint);
                        }
                        
                        // Store the account structures for later use in sell transactions
                        stored_accounts.boop_fun_accounts = Some(boop_fun_accounts);
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON] DEBUG - Stored pump_fun_accounts: {:?}", stored_accounts.pump_swap_accounts);
                        }
                        
                        // PERFORMANCE: Checkpoint 4 - PumpFun builder
                        let now = std::time::Instant::now();
                        phase_timings.insert("boop_fun_builder", now.duration_since(last_checkpoint));
                        last_checkpoint = now;
                        
                        break;
                    }
                                            
                }

                ProgramType::RaydiumLaunchpad => {
                    // Check discriminator for Raydium
                    if instruction.data.len() > 8 && &instruction.data[0..8] == [250, 234, 13, 123, 213, 156, 19, 236] {
                        println!("[{}] - [TRITON] Processing Raydium launchpad instruction", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                        
                        // Convert account keys to Vec<Vec<u8>> format expected by the function
                        let account_keys_bytes: Vec<Vec<u8>> = account_keys.iter()
                            .map(|pk| pk.to_bytes().to_vec())
                            .collect();
                        
                        // Convert account metas to Vec<u8> format expected by the function
                        let accounts_bytes: Vec<u8> = instruction.accounts.iter()
                            .map(|meta| meta.pubkey.to_bytes().to_vec())
                            .flatten()
                            .collect();
                        
                        // DEBUG: Show what's being passed to the function
                        println!("[TRITON] DEBUG - For Raydium Launchpad instruction:");
                        println!("[TRITON] DEBUG - account_keys_bytes ({} total):", account_keys_bytes.len());
                        for (i, key_bytes) in account_keys_bytes.iter().enumerate() {
                            let pubkey = solana_sdk::pubkey::Pubkey::try_from(key_bytes.as_slice()).unwrap_or_default();
                            println!("[TRITON] DEBUG -   Account {}: {} ({} bytes)", i, pubkey, key_bytes.len());
                        }
                        println!("[TRITON] DEBUG - accounts_bytes ({} total bytes):", accounts_bytes.len());
                        for (i, account_meta) in instruction.accounts.iter().enumerate() {
                            println!("[TRITON] DEBUG -   Instruction Account {}: pubkey={}, is_signer={}, is_writable={}", 
                                i, account_meta.pubkey, account_meta.is_signer, account_meta.is_writable);
                        }
                        println!("[TRITON] DEBUG - End Raydium Launchpad debug info");

                        
                        let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                        
                        // Convert sig_bytes to Arc<Vec<u8>> format
                        let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                        
                        // Use parsed amounts if available, otherwise fall back to config
                        let sol_amount_to_use = parsed.sol_buy_amount_lamports.unwrap_or(buy_sol_lamports);
                        let mint_amount_to_use = parsed.mint_token_amount.unwrap_or(0);
                        println!("[TRITON] DEBUG - Raydium: Using amounts: SOL={} lamports (parsed: {:?}, config: {}), Mint={} (parsed: {:?})", 
                            sol_amount_to_use, parsed.sol_buy_amount_lamports, buy_sol_lamports, mint_amount_to_use, parsed.mint_token_amount);
                        
                        let (instruction_result, mint_val, target_val, _accounts) = raydium_launchpad_build_buy_tx(
                            &account_keys_bytes,
                            &accounts_bytes,
                            sig_bytes_arc,
                            detection_time,
                            &instruction.data,
                            sol_amount_to_use,
                            config.buy_slippage_bps,
                        );
                        
                        buy_instruction = instruction_result;
                        mint = mint_val;
                        target_token_buy = target_val;
                        tx_type = "ray_launch".to_string();
                        send_tx = true;
                        println!("[{}] - [TRITON] Raydium launchpad instruction built successfully", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                        break;
                    }
                },
                // ProgramType::AxiomPumpSwap => {
                //     println!("[{}] - [TRITON] Processing Axiom pump swap instruction", 
                //         Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                    
                //     let account_keys_bytes: Vec<Vec<u8>> = account_keys.iter()
                //         .map(|pk| pk.to_bytes().to_vec())
                //         .collect();
                    
                //     let accounts_bytes: Vec<u8> = instruction.accounts.iter()
                //         .map(|meta| meta.pubkey.to_bytes().to_vec())
                //         .flatten()
                //         .collect();
                    
                //     // DEBUG: Show what's being passed to the function
                //     println!("[TRITON] DEBUG - For Axiom Pump Swap instruction:");
                //     println!("[TRITON] DEBUG - account_keys_bytes ({} total):", account_keys_bytes.len());
                //     for (i, key_bytes) in account_keys_bytes.iter().enumerate() {
                //         let pubkey = solana_sdk::pubkey::Pubkey::try_from(key_bytes.as_slice()).unwrap_or_default();
                //         println!("[TRITON] DEBUG -   Account {}: {} ({} bytes)", i, pubkey, key_bytes.len());
                //     }
                //     println!("[TRITON] DEBUG - accounts_bytes ({} total bytes):", accounts_bytes.len());
                //     for (i, account_meta) in instruction.accounts.iter().enumerate() {
                //         println!("[TRITON] DEBUG -   Instruction Account {}: pubkey={}, is_signer={}, is_writable={}", 
                //             i, account_meta.pubkey, account_meta.is_signer, account_meta.is_writable);
                //     }
                //     println!("[TRITON] DEBUG - End Axiom Pump Swap debug info");

                    
                //     let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                    
                //     // Convert sig_bytes to Arc<Vec<u8>> format
                //     let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                    
                //     let (instruction_result, mint_val, target_val, _accounts) = axiom_pump_swap_build_buy_tx(
                //         &account_keys_bytes,
                //         &accounts_bytes,
                //         sig_bytes_arc,
                //         detection_time,
                //         buy_sol_lamports,
                //         config.buy_slippage_bps,
                //     );
                    
                //     buy_instruction = instruction_result;
                //     mint = mint_val;
                //     target_token_buy = target_val;
                //     tx_type = "pump_swap".to_string();
                //     send_tx = true;
                //     println!("[{}] - [TRITON] Axiom pump swap instruction built successfully", 
                //         Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                //     break;
                // },
                // ProgramType::AxiomPumpFun => {
                //     println!("[{}] - [TRITON] Processing Axiom pump fun instruction", 
                //         Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                    
                //     // DEBUG: Show what's being passed to the function
                //     println!("[TRITON] DEBUG - For Axiom pump fun instruction:");
                //     println!("[TRITON] DEBUG - instruction.accounts ({} total):", instruction.accounts.len());
                //     for (i, account_meta) in instruction.accounts.iter().enumerate() {
                //         println!("[TRITON] DEBUG -   Instruction Account {}: pubkey={}, is_signer={}, is_writable={}", 
                //             i, account_meta.pubkey, account_meta.is_signer, account_meta.is_writable);
                //     }
                //     println!("[TRITON] DEBUG - End Axiom pump fun debug info");

                //     // let mint_pubkey = solana_sdk::pubkey::Pubkey::try_from(instruction.accounts[2].as_slice()).unwrap_or_default();
                //     println!("mint: {:?}", instruction.accounts[2].pubkey);
                    
                //     let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                    
                //     // Convert sig_bytes to Arc<Vec<u8>> format
                //     let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                    
                //     let (instruction_result, mint_val, target_val, _accounts) = axiom_pump_fun_build_buy_tx(
                //         &instruction.accounts,
                //         sig_bytes_arc,
                //         detection_time,
                //         buy_sol_lamports,
                //         config.buy_slippage_bps,
                //     );
                    
                //     buy_instruction = instruction_result;
                //     mint = mint_val;
                //     target_token_buy = target_val;
                //     tx_type = "pumpfun".to_string();
                //     send_tx = true;
                //     println!("[{}] - [TRITON] Axiom pump fun instruction built successfully", 
                //         Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                //     break;
                // },
                ProgramType::RaydiumCpmm => {
                    println!("[{}] - [TRITON] Processing Raydium CPMM instruction", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                    
                    let account_keys_bytes: Vec<Vec<u8>> = account_keys.iter()
                        .map(|pk| pk.to_bytes().to_vec())
                        .collect();
                    
                    let accounts_bytes: Vec<u8> = instruction.accounts.iter()
                        .map(|meta| meta.pubkey.to_bytes().to_vec())
                        .flatten()
                        .collect();
                    
                    // DEBUG: Show what's being passed to the function
                    println!("[TRITON] DEBUG - For Raydium CPMM instruction:");
                    println!("[TRITON] DEBUG - account_keys_bytes ({} total):", account_keys_bytes.len());
                    for (i, key_bytes) in account_keys_bytes.iter().enumerate() {
                        let pubkey = solana_sdk::pubkey::Pubkey::try_from(key_bytes.as_slice()).unwrap_or_default();
                        println!("[TRITON] DEBUG -   Account {}: {} ({} bytes)", i, pubkey, key_bytes.len());
                    }
                    println!("[TRITON] DEBUG - accounts_bytes ({} total bytes):", accounts_bytes.len());
                    for (i, account_meta) in instruction.accounts.iter().enumerate() {
                        println!("[TRITON] DEBUG -   Instruction Account {}: pubkey={}, is_signer={}, is_writable={}", 
                            i, account_meta.pubkey, account_meta.is_signer, account_meta.is_writable);
                    }
                    println!("[TRITON] DEBUG - End Raydium CPMM debug info");

                    
                    let detection_time = parsed.detection_time.unwrap_or_else(Instant::now);
                    
                    // Convert sig_bytes to Arc<Vec<u8>> format
                    let sig_bytes_arc = parsed.sig_bytes.as_ref().map(|s| std::sync::Arc::new(s.clone()));
                    
                    let (instruction_result, mint_val, target_val, _accounts) = raydium_cpmm_build_buy_tx(
                        &account_keys_bytes,
                        &accounts_bytes,
                        sig_bytes_arc,
                        detection_time,
                        buy_sol_lamports,
                        config.buy_slippage_bps,
                    );
                    
                    buy_instruction = instruction_result;
                    mint = mint_val;
                    target_token_buy = target_val;
                    tx_type = "ray_cpmm".to_string();
                    send_tx = true;
                    println!("[{}] - [TRITON] Raydium CPMM instruction built successfully", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                    break;
                },
            }
        }
    }
    
    if !send_tx {
        eprintln!("[ERROR] No supported instruction found for transaction");
        return;
    }
    
    println!("[{}] - [TRITON] Building vendor transactions for {} (mint: {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), tx_type, mint);
    
    // PERFORMANCE: Checkpoint 5 - Before vendor building
    let now = std::time::Instant::now();
    phase_timings.insert("pre_vendor_build", now.duration_since(last_checkpoint));
    last_checkpoint = now;
    
    // Build vendor-specific transactions in parallel
    let build_result = crate::build_tx::tx_builder::build_vendor_specific_transactions_parallel(
        buy_instruction,
        mint,
        target_token_buy,
        &sig_string, // sig_str for logging
        token_2022,
    );
    
    // PERFORMANCE: Checkpoint 6 - After vendor building
    let now = std::time::Instant::now();
    phase_timings.insert("vendor_build", now.duration_since(last_checkpoint));
    last_checkpoint = now;
    
    match build_result {
        Ok(vendor_transactions) => {
            if !vendor_transactions.is_empty() {
                println!("[{}] - [TRITON] SUCCESS - Built {} vendor transactions for sig: {}", 
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), vendor_transactions.len(), sig_string);
                
                // Send all vendor transactions in parallel
                let detection_time = match parsed.detection_time {
                    Some(time) => time,
                    None => {
                        eprintln!("[ERROR] No detection time found for signature: {}", sig_string);
                        return;
                    }
                };
                
                // CRITICAL FIX: Send transactions and store info for ALL signatures (not just winning one)
                // This handles the nonce issue where each vendor gets different signatures
                ASYNC_RUNTIME.spawn(async move {
                    // Clone the data we need for storing transaction info
                    let tx_type_clone = tx_type.clone();
                    let mint_clone = mint;
                    let target_token_buy_clone = target_token_buy;
                    let parsed_sol_amount = parsed.sol_buy_amount_lamports.unwrap_or(buy_sol_lamports);
                    let parsed_instructions_clone = parsed.instructions.clone();
                    let parsed_account_keys_clone = parsed.account_keys.clone();
                    let parsed_feed_id_clone = parsed.feed_id.clone();
                    let parsed_slot = parsed.slot.unwrap_or(0);
                    let stored_accounts_clone = stored_accounts.clone();
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON] DEBUG - About to send transactions. stored_accounts: {:?}", stored_accounts_clone);
                    }
                    
                    // PERFORMANCE TRACKING: Calculate timing metrics
                    let tx_build_time = tx_build_start.elapsed();
                    
                    // Calculate detection delay if we have parser timing info
                    let detection_delay = if let (Some(parser_first_hit), Some(grpc_creation_time)) = (parsed.parser_first_hit, parsed.detection_time) {
                        // DEBUG: Show the actual timestamps being compared
                        println!("[TRITON] DEBUG - Performance timing breakdown:");
                        println!("[TRITON] DEBUG -   parser_first_hit: {:?}", parser_first_hit);
                        println!("[TRITON] DEBUG -   grpc_creation_time: {:?}", grpc_creation_time);
                        println!("[TRITON] DEBUG -   Current time: {:?}", std::time::Instant::now());
                        
                        let delay = parser_first_hit.duration_since(grpc_creation_time);
                        println!("[TRITON] DEBUG -   Calculated delay: {:.3?} ({}ns)", delay, delay.as_nanos());
                        delay
                    } else {
                        println!("[TRITON] DEBUG - Missing timing info: parser_first_hit={:?}, detection_time={:?}", 
                            parsed.parser_first_hit, parsed.detection_time);
                        std::time::Duration::ZERO
                    };
                    
                    // Calculate total time from GRPC creation to current moment
                    let total_time = if let Some(grpc_creation_time) = parsed.detection_time {
                        std::time::Instant::now().duration_since(grpc_creation_time)
                    } else {
                        tx_build_time // Fallback to build time if no GRPC timestamp
                    };
                    
                    // Send all vendor transactions and capture ALL successful signatures
                    let send_result = send_and_store_all_signatures(
                        &vendor_transactions, 
                        detection_time, 
                        tx_type_clone, 
                        mint_clone, 
                        target_token_buy_clone, 
                        parsed_sol_amount, 
                        parsed_instructions_clone, 
                        parsed_account_keys_clone, 
                        parsed_feed_id_clone, 
                        parsed_slot,
                        stored_accounts_clone.pump_fun_accounts,
                        stored_accounts_clone.pump_swap_accounts,
                        stored_accounts_clone.ray_launch_accounts,
                        stored_accounts_clone.raydium_cpmm_accounts,
                        stored_accounts_clone.heaven_accounts,
                        stored_accounts_clone.boop_fun_accounts,
                    ).await;
                    
                    match send_result {
                        Ok((winning_vendor, winning_sig)) => {
                            println!("[{}] - [TRITON] PARALLEL SUCCESS - {} won with sig: {}", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), winning_vendor, winning_sig);
                            
                            // Transaction info for ALL signatures has already been stored by send_and_store_all_signatures
                            println!("[{}] - [TRITON] Transaction info stored for ALL successful signatures (handling nonce variations)", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                        }
                        Err(e) => {
                            eprintln!("[{}] - [TRITON] ERROR - Parallel send failed for sig: {} - Error: {:?}", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, e);
                        }
                    }
                    
                    // PERFORMANCE LOGGING: Moved here to avoid slowing down the critical send operation
                    let tx_signature = parsed.sig_bytes.as_ref()
                        .map(|sig| bs58::encode(sig).into_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    
                    println!("[TRITON] PERFORMANCE - Buy TX Performance Metrics for sig: {}", tx_signature);
                    println!("[TRITON] PERFORMANCE -   Detection Delay: {:.3?} ({}ns) (GRPC creation → parser first hit)", 
                        detection_delay, detection_delay.as_nanos());
                    println!("[TRITON] PERFORMANCE -   Parse Time: {:.3?} ({}ns) (parser first hit → program type match)", 
                        parsed.parse_time.unwrap_or_default(), parsed.parse_time.unwrap_or_default().as_nanos());
                    println!("[TRITON] PERFORMANCE -   Tx Build Time: {:.3?} ({}ns) (program type match → send)", 
                        tx_build_time, tx_build_time.as_nanos());
                    println!("[TRITON] PERFORMANCE -   Total Time: {:.3?} ({}ns) (GRPC creation → send)", 
                        total_time, total_time.as_nanos());
                    
                    // PERFORMANCE: Show detailed phase breakdown
                    println!("[TRITON] PERFORMANCE - Detailed Build Phase Breakdown:");
                    for (phase, duration) in &phase_timings {
                        println!("[TRITON] PERFORMANCE -     {}: {:.3?} ({}ns)", 
                            phase, duration, duration.as_nanos());
                    }
                });
            } else {
                eprintln!("[{}] - [TRITON] ERROR - No vendor transactions built for sig: {}", 
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string);
            }
        }
        Err(e) => {
            eprintln!("[{}] - [TRITON] ERROR - Failed to build vendor transactions for sig: {} - Error: {:?}", 
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, e);
        }
    }
}