use crossbeam::channel::{unbounded, Sender};
use once_cell::sync::OnceCell;
use bs58;
use core_affinity;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::system_program;
use solana_sdk::transaction::Transaction;
use solana_sdk::instruction::Instruction;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use chrono::Utc;
use crate::utils::logger::{log_event, setup_event_logger, EventType};
use tokio::time::{sleep, Duration};
use crate::grpc::arpc_parser::GLOBAL_TX_MAP;
use crate::build_tx::pump_fun::{build_sell_instruction, BondingCurve};
use crate::init::wallet_loader::get_wallet_keypair;
use crate::build_tx::pump_swap::build_pump_sell_instruction_raw;
use crate::build_tx::ray_launch::build_ray_launch_sell_instruction;
use crate::build_tx::ray_cpmm::{build_ray_cpmm_sell_instruction, build_ray_cpmm_sell_instruction_no_quote};
use crate::send_tx::rpc::send_tx_via_send_rpcs;
use crate::send_tx::zero_slot::{create_instruction_zeroslot, send_tx_zeroslot};
use crate::build_tx::tx_builder::{build_and_sign_transaction, create_instruction};
use solana_sdk::signature::Signer;
use crate::config_load::GLOBAL_CONFIG;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use borsh::BorshDeserialize;
use std::time::Instant;

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
    std::thread::spawn(move || {
        while let Ok(parsed) = rx.recv() {
            // Heavy processing here (sync, fast)
            let sig_detect = if let Some(sig) = parsed.sig_bytes.clone() {
                bs58::encode(sig).into_string()
            } else {
                String::new()
            };
            let config = GLOBAL_CONFIG.get().expect("Config not initialized");

            // println!("[crossbeam worker] Received: {:?}, sig_bytes: {:?}", parsed, sig_detect);
            let mut found = None;
            if parsed.is_signer {
                for entry in GLOBAL_TX_MAP.iter() {
                    if entry.value().send_sig.trim_matches('\"') == sig_detect {
                        found = Some(entry.value().clone()); // or entry.key().clone(), or both
                        break;
                    }
                }
                if let Some(tx_with_pubkey) = found {
                    let now = Utc::now();
                    let mut send_tx: bool = false;

                    sleep(Duration::from_secs(4));
                    let mut sell_instruction: Instruction = Instruction{
                        program_id: Pubkey::new_unique(),
                        accounts: vec![],
                        data: vec![],
                    };
                    let mut tx_type = tx_with_pubkey.tx_type;

                    //check if pumpfun token has migrated or not, if true, switch to pumpswap sell logic
                    let rpc: &solana_client::rpc_client::RpcClient = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
                    
                    if tx_type == "pumpfun" {
                        println!("bonding curve: {:?}", tx_with_pubkey.bonding_curve);
                        let account_data = rpc.get_account_data(&tx_with_pubkey.bonding_curve).unwrap();
                        let bonding_curve_state = BondingCurve::deserialize(&mut &account_data[8..]).unwrap();
                        if bonding_curve_state.complete {
                            tx_type = "pumpswap".to_string();
                            println!("[{}] - [grpc] Pumpfun token has migrated to pumpswap - applying pumpswap sell logic", now.format("%Y-%m-%d %H:%M:%S%.3f"));
                        }
                    }

                    if tx_type == "raylaunch" {
                        let pool_state = tx_with_pubkey.ray_launch_accounts.pool_state;
                        let res = rpc.get_account_data(&pool_state).unwrap();
                        let status = res[17];
                        let migrate = res[20];
                        
                        if status > 0 {
                            tx_type = "raylaunch_complete".to_string();
                            if migrate == 1 {
                                println!("[{}] - [grpc] Raylaunch pool is complete - applying Raydium CPMM sell logic", now.format("%Y-%m-%d %H:%M:%S%.3f"));
                                tx_type = "ray_launch_cpmm".to_string();
                            }
                        }
                    }

                    if tx_type == "pumpfun" {
                        sell_instruction = build_sell_instruction(
                            get_wallet_keypair().pubkey(),
                            tx_with_pubkey.mint,
                            tx_with_pubkey.bonding_curve,
                            tx_with_pubkey.token_amount,
                            config.sell_slippage_bps,
                        )
                        .unwrap();
                        send_tx = true;
                    }
                    if tx_type == "pumpswap" {
                        sell_instruction = build_pump_sell_instruction_raw(
                            tx_with_pubkey.token_amount,
                            config.sell_slippage_bps,
                            tx_with_pubkey.mint,
                        );
                        send_tx = true;
                    }
                    if tx_type == "raylaunch" {
                        sell_instruction = build_ray_launch_sell_instruction(
                            tx_with_pubkey.token_amount,
                            config.sell_slippage_bps,
                            tx_with_pubkey.ray_launch_accounts,
                        );
                        send_tx = true;
                    }
                    if tx_type == "ray_cpmm" {
                        sell_instruction = build_ray_cpmm_sell_instruction_no_quote(
                            tx_with_pubkey.ray_cpmm_pool_state,
                            tx_with_pubkey.ray_cpmm_accounts,
                            tx_with_pubkey.token_amount,
                        );
                        send_tx = true;
                    }
                    if tx_type == "ray_launch_cpmm" {
                        sell_instruction = build_ray_cpmm_sell_instruction(
                            tx_with_pubkey.token_amount,
                            config.sell_slippage_bps,
                            tx_with_pubkey.mint,
                        );
                        send_tx = true;
                    }

                    if send_tx {
                        let compute_budget_instruction = create_instruction(
                            config.cu_limit,
                            config.cu_price0_slot,
                            tx_with_pubkey.mint,
                            vec![sell_instruction.clone()],
                        );
                        // let final_instruction = create_instruction_nextblock(compute_budget_instruction,  (config.nextblock_sell_tip * 1_000_000_000.0) as u64);
                        let final_instruction = create_instruction_zeroslot(compute_budget_instruction,  (config.zeroslot_sell_tip * 1_000_000_000.0) as u64);
                        let tx = build_and_sign_transaction(
                            rpc,
                            &final_instruction,
                            get_wallet_keypair(),
                        )
                        .ok();
                        // println!("Signed tx, elapsed: {:.2?}", start_time.elapsed());
                        // let sig = send_tx_nextblock(&tx.unwrap(), &config.nextblock_api)
                        //     .await
                        //     .unwrap();
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        let sig = rt.block_on(send_tx_zeroslot(&tx.unwrap())).unwrap();
                        println!(
                            "[{}] - sell tx sent with sig: {}",
                            now.format("%Y-%m-%d %H:%M:%S%.3f"),
                            sig
                        );
                    }
                }

            } else { //send tx
                if let Some(mut tx_with_pubkey) = GLOBAL_TX_MAP.get_mut(&sig_detect) {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let sig = rt.block_on(send_tx_zeroslot(&tx_with_pubkey.tx)).unwrap();
                    let now = Utc::now();
                    println!(
                        "[{}] - Sent tx with sig: {} | elapsed: {:.2?}",
                        now.format("%Y-%m-%d %H:%M:%S%.3f"),
                        sig,
                        parsed.detection_time.unwrap().elapsed()
                    );
                    tx_with_pubkey.send_sig = sig.clone();
                }
            }        }
    });
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
