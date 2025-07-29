use crossbeam::channel::{unbounded, Sender};
use once_cell::sync::OnceCell;
use bs58;
use core_affinity;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::Instruction;
use chrono::Utc;
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


// Add global counters for monitoring triton worker performance
use std::sync::atomic::{AtomicUsize, Ordering};
static TRITON_MESSAGES_RECEIVED: AtomicUsize = AtomicUsize::new(0);
static TRITON_TRANSACTIONS_SENT: AtomicUsize = AtomicUsize::new(0);
static TRITON_TRANSACTIONS_FOUND: AtomicUsize = AtomicUsize::new(0);
static TRITON_ERRORS: AtomicUsize = AtomicUsize::new(0);

pub fn get_triton_stats() -> (usize, usize, usize, usize) {
    (
        TRITON_MESSAGES_RECEIVED.load(Ordering::Relaxed),
        TRITON_TRANSACTIONS_SENT.load(Ordering::Relaxed),
        TRITON_TRANSACTIONS_FOUND.load(Ordering::Relaxed),
        TRITON_ERRORS.load(Ordering::Relaxed),
    )
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
    // Add more fields as needed
}

static PARSED_TX_SENDER: OnceCell<Sender<ParsedTx>> = OnceCell::new();

/// Call this once at startup (e.g., in main.rs) to spawn the worker thread.
pub fn setup_crossbeam_worker() {
    let (tx, rx) = unbounded::<ParsedTx>();
    PARSED_TX_SENDER.set(tx).unwrap();
    
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
            
            while let Ok(parsed) = rx_clone.recv() {
            TRITON_MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);
            
            // Heavy processing here (sync, fast)
            let sig_detect = if let Some(sig) = parsed.sig_bytes.clone() {
                bs58::encode(sig).into_string()
            } else {
                String::new()
            };

            let now = Utc::now();
            println!("[{}] - [TRITON-{}] Processing message for sig: {} (total received: {})", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), 
                worker_id,
                sig_detect, 
                TRITON_MESSAGES_RECEIVED.load(Ordering::Relaxed));

            if parsed.detection_time.is_none() {
                TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                eprintln!("[crossbeam_worker] Error: detection_time is None for sig_detect={}", sig_detect);
            }

            let config = match GLOBAL_CONFIG.get() {
                Some(cfg) => cfg,
                None => {
                    eprintln!("[crossbeam_worker] Error: Config not initialized");
                    continue;
                }
            };

            // println!("[crossbeam worker] Received: {:?}, sig_bytes: {:?}", parsed, sig_detect);
            let mut found = None;
            if parsed.is_signer {
                let map_size = GLOBAL_TX_MAP.len();
                println!("[{}] - [TRITON-{}] Searching GLOBAL_TX_MAP for sig: {} (map size: {})", 
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_detect, map_size);
                
                for entry in GLOBAL_TX_MAP.iter() {
                    if entry.value().send_sig.trim_matches('\"') == sig_detect {
                        found = Some(entry.value().clone()); // or entry.key().clone(), or both
                        TRITON_TRANSACTIONS_FOUND.fetch_add(1, Ordering::Relaxed);
                        println!("[{}] - [TRITON-{}] FOUND transaction in map for sig: {} (tx_type: {})", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_detect, entry.value().tx_type);
                        break;
                    }
                }
                
                if found.is_none() {
                    println!("[{}] - [TRITON-{}] NOT FOUND transaction in map for sig: {} (map size: {})", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_detect, map_size);
                }
                if let Some(mut tx_with_pubkey) = found {
                    let now = Utc::now();
                    let mut send_tx: bool = false;

                    // let sig_detect = if let Some(sig) = parsed.sig_bytes.clone() {
                    //     bs58::encode(sig).into_string()
                    // } else {
                    //     String::new()
                    // };
                    let sig_bytes = parsed.sig_bytes.as_ref().unwrap();
                    log_event(
                        EventType::GrpcLanded,
                        sig_bytes,
                        tx_with_pubkey.send_time,
                        Some((parsed.slot.unwrap() - tx_with_pubkey.send_slot) as i64)
                    );

                    // Use configurable wait time instead of hardcoded 4 seconds
                    let wait_time_secs = config.wait_time as u64;
                    thread::sleep(Duration::from_secs(wait_time_secs));
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
                                println!("[{}] - [grpc] Pumpfun token has migrated to pumpswap - applying pumpswap sell logic", now.format("%Y-%m-%d %H:%M:%S%.3f"));
                                tx_with_pubkey.pump_swap_accounts = Some(GLOBAL_MONITORING_DATA.get(&tx_with_pubkey.mint).unwrap().pump_fun_accounts.clone());
                                //need to figure out how to build pump swap struct!!!!!!!!!!!!!
                            }
                        }
                    }

                    if tx_type == "ray_launch" {
                        if let Some(ray_launch_accounts) = &tx_with_pubkey.ray_launch_accounts {
                            let pool_state = ray_launch_accounts.pool_state;
                            let res = match rpc.get_account_data(&pool_state) {
                                Ok(data) => data,
                                Err(e) => {
                                    eprintln!("[crossbeam_worker] Error: get_account_data (raylaunch) failed: {:?}", e);
                                    continue;
                                }
                            };
                            let status = res[17];
                            let migrate = res[20];
                            
                            if status > 0 {
                                // tx_type = "ray_cpmm".to_string();
                                if migrate == 1 {
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
                        println!("[{}] - [TRITON] Building sell transaction for sig: {} (tx_type: {})", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect, tx_type);
                        
                        // Build vendor-specific sell transactions in parallel using the same function as buy
                        match crate::build_tx::tx_builder::build_vendor_specific_transactions_parallel(
                            sell_instruction,
                            tx_with_pubkey.mint,
                            0, // target_token_buy not used for sell transactions
                            &sig_detect, // sig_str for logging
                        ) {
                            Ok(vendor_transactions) => {
                                if !vendor_transactions.is_empty() {
                                    println!("[{}] - [TRITON] SUCCESS - Built {} vendor sell transactions for sig: {}", 
                                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), vendor_transactions.len(), sig_detect);
                                    
                                    // Send all vendor transactions in parallel
                                    let sig_detect_clone = sig_detect.clone();
                                    let sig_bytes_clone = sig_bytes.clone();
                                    let detection_time = parsed.detection_time.unwrap();
                                    
                                    ASYNC_RUNTIME.spawn(async move {
                                        match send_all_vendors_parallel(&vendor_transactions, detection_time).await {
                                            Ok((winning_vendor, sig)) => {
                                                TRITON_TRANSACTIONS_SENT.fetch_add(1, Ordering::Relaxed);
                                                println!(
                                                    "[{}] - [TRITON] PARALLEL SELL SUCCESS - {} won with sig: {} | total sent: {}",
                                                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                                                    winning_vendor,
                                                    sig,
                                                    TRITON_TRANSACTIONS_SENT.load(Ordering::Relaxed)
                                                );
                                                // Remove the processed transaction from GLOBAL_TX_MAP to prevent memory leaks
                                                GLOBAL_TX_MAP.remove(&sig_bytes_clone);
                                            }
                                            Err(e) => {
                                                TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                                eprintln!("[{}] - [TRITON] ERROR - Parallel sell send failed for sig: {} - Error: {:?}", 
                                                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect_clone, e);
                                            }
                                        }
                                    });
                                } else {
                                    TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                    eprintln!("[{}] - [TRITON] ERROR - No vendor sell transactions built for sig: {}", 
                                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect);
                                }
                            }
                            Err(e) => {
                                TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                eprintln!("[{}] - [TRITON] ERROR - Failed to build vendor sell transactions for sig: {} - Error: {:?}", 
                                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect, e);
                            }
                        }
                    } else {
                        println!("[{}] - [TRITON] No sell transaction to build for sig: {} (tx_type: {})", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect, tx_type);
                    }
                }

            } else { //send tx
                println!("[{}] - [TRITON] Attempting to send buy transaction for sig: {}", 
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_detect);
                
                if let Some(sig_bytes) = &parsed.sig_bytes {
                    if let Some(mut tx_with_pubkey) = GLOBAL_TX_MAP.get_mut(sig_bytes) {
                        // Get vendor transactions for parallel sending
                        let vendor_transactions = tx_with_pubkey.vendor_transactions.clone();
                        let detection_time = parsed.detection_time.unwrap();
                        let slot = parsed.slot.unwrap();
                        let sig_bytes_clone = sig_bytes.clone();
                        
                        // Update the transaction info immediately (non-blocking)
                        tx_with_pubkey.send_time = Instant::now();
                        
                        ASYNC_RUNTIME.spawn(async move {
                            match send_all_vendors_parallel(&vendor_transactions, detection_time).await {
                                Ok((winning_vendor, sig)) => {
                                    TRITON_TRANSACTIONS_SENT.fetch_add(1, Ordering::Relaxed);
                                    
                                    println!(
                                        "[{}] - [TRITON] PARALLEL SUCCESS - {} won with sig: {} | total sent: {}",
                                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                                        winning_vendor,
                                        sig,
                                        TRITON_TRANSACTIONS_SENT.load(Ordering::Relaxed)
                                    );
                                    
                                    // Update the transaction info in the map (this is safe since we're not blocking)
                                    if let Some(mut tx_with_pubkey) = GLOBAL_TX_MAP.get_mut(&sig_bytes_clone) {
                                        tx_with_pubkey.send_sig = sig.clone();
                                        tx_with_pubkey.send_slot = slot;
                                        println!("[{}] - [TRITON] Saved sig: {} to GLOBAL_TX_MAP", Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig);
                                    }
                                }
                                Err(e) => {
                                    TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                                    eprintln!("[crossbeam_worker] Error: Parallel send failed: {:?}", e);
                                }
                            }
                        });
                    } else {
                        TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                        eprintln!("[crossbeam_worker] Error: GLOBAL_TX_MAP.get_mut failed for sig_detect={}", sig_detect);
                    }
                } else {
                    TRITON_ERRORS.fetch_add(1, Ordering::Relaxed);
                    eprintln!("[crossbeam_worker] Error: No sig_bytes found for sig_detect={}", sig_detect);
                }
            }
        }
    });
    }
}

/// Call this from your parser to send a parsed message to the worker.
pub fn send_parsed_tx(parsed: ParsedTx) {
    if let Some(sender) = PARSED_TX_SENDER.get() {
        let _ = sender.send(parsed);
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
