use crate::build_tx::pump_fun::{build_sell_instruction, BondingCurve};
use crate::build_tx::tx_builder::{build_and_sign_transaction, create_instruction};
use crate::config_load::GLOBAL_CONFIG;
use crate::geyser::{subscribe_update::UpdateOneof, SubscribeUpdate};
use crate::grpc::arpc_parser::GLOBAL_TX_MAP;
use crate::init::initialize::GLOBAL_RPC_CLIENT; // or wherever you defined it
use crate::send_tx::nextblock::send_tx_nextblock;
use chrono::Utc;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signer;
use tokio::time::{sleep, Duration};
use crate::init::wallet_loader::get_wallet_keypair;
use solana_program::instruction::{Instruction};
use solana_sdk::pubkey::Pubkey;
use crate::build_tx::pump_swap::build_pump_sell_instruction_raw;
use crate::send_tx::nextblock::create_instruction_nextblock;
use borsh::BorshDeserialize;
use crate::build_tx::ray_launch::build_ray_launch_sell_instruction;
use crate::build_tx::ray_cpmm::{build_ray_cpmm_sell_instruction, build_ray_cpmm_sell_instruction_with_pool_state};
use crate::send_tx::rpc::send_tx_via_send_rpcs;
use crate::send_tx::zero_slot::{create_instruction_zeroslot, send_tx_zeroslot};
use std::time::Instant;
use crate::utils::logger::{log_event, setup_event_logger, EventType};
use crossbeam::channel::{unbounded, Sender};
use once_cell::sync::OnceCell;
use crate::triton_grpc::crossbeam_worker::{ParsedTx, send_parsed_tx};
use core_affinity;


// Pin the parsing thread to core 1 for lowest latency
pub fn process_triton_message(resp: &SubscribeUpdate) {
    // Core pinning removed; now handled in client.rs
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");

    if let Some(update) = &resp.update_oneof {
        match update {
            UpdateOneof::Transaction(tx_update) => {
                let start_time = Instant::now();

                if let Some(tx_info) = &tx_update.transaction {
                    if let Some(tx) = &tx_info.transaction {
                        let sig_bytes = tx.signatures.get(0).map(|s| s.clone());
                        let wallet_pubkey = get_wallet_keypair().pubkey();
                        let is_signer = if let Some(message) = &tx.message {
                            if let Some(header) = &message.header {
                                let num_signers = header.num_required_signatures as usize;
                                message.account_keys
                                    .iter()
                                    .take(num_signers)
                                    .any(|bytes| {
                                        let pubkey = unsafe {
                                            Pubkey::new_from_array(*(bytes.as_ptr() as *const [u8; 32]))
                                        };
                                        pubkey == wallet_pubkey
                                    })
                            } else { false }
                        } else { false };
                        // Handoff to crossbeam worker for heavy processing
                        log_event(EventType::GrpcDetectionProcessing, "&sig_detect", start_time, None);
                        let parsed = ParsedTx {
                            sig_bytes,
                            is_signer,
                            slot: Some(tx_update.slot),
                            detection_time: Some(start_time),
                        };
                        send_parsed_tx(parsed);
                        return;
                    }
                }
            }
            // The following arms are commented out for speed. Uncomment if needed for debugging or additional features.
            // UpdateOneof::Slot(slot_info) => {
            //     let now = Utc::now();
            //     println!(
            //         "[{}] - [Triton] Slot Update: slot={}, status={:?}",
            //         now.format("%Y-%m-%d %H:%M:%S%.3f"),
            //         slot_info.slot,
            //         slot_info.status()
            //     );
            // }
            // UpdateOneof::Account(account_info) => {
            //     let now = Utc::now();
            //     println!(
            //         "[{}] - [Triton] Account Update: {:?}",
            //         now.format("%Y-%m-%d %H:%M:%S%.3f"),
            //         account_info.account
            //     );
            // }
            // UpdateOneof::Block(block_info) => {
            //     // let now = Utc::now();
            //     // println!("[{}] - [Triton] Block Update: slot={}", now.format("%Y-%m-%d %H:%M:%S%.3f"), block_info.slot);
            // }
            // UpdateOneof::Ping(_) => {
            //     // let now = Utc::now();
            //     // println!(
            //     //     "[{}] - [Triton] Received ping.",
            //     //     now.format("%Y-%m-%d %H:%M:%S%.3f")
            //     // );
            // }
            // UpdateOneof::Pong(_) => {
            //     // let now = Utc::now();
            //     // println!(
            //     //     "[{}] - [Triton] Received pong.",
            //     //     now.format("%Y-%m-%d %H:%M:%S%.3f")
            //     // );
            // }
            // _ => {
            //     let now = Utc::now();
            //     println!(
            //         "[{}] - [Triton] Received other message type: {:?}",
            //         now.format("%Y-%m-%d %H:%M:%S%.3f"),
            //         update
            //     );
            // }
            _ => {}
        }
    }
}

pub fn get_blockhash() -> Hash {
    let rpc = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    let blockhash = match rpc.get_latest_blockhash() {
        Ok(hash) => hash,
        Err(e) => {
            // handle error
            return Hash::default();
        }
    };
    blockhash
}
