use crate::config_load::GLOBAL_CONFIG;
use crate::geyser::{subscribe_update::UpdateOneof, SubscribeUpdate};
use crate::init::initialize::GLOBAL_RPC_CLIENT; // or wherever you defined it
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signer;
use crate::init::wallet_loader::get_wallet_keypair;
use std::time::Instant;
use crate::utils::logger::{log_event, EventType};
use crate::triton_grpc::crossbeam_worker::{ParsedTx, send_parsed_tx};

// Pin the parsing thread to core 0 for lowest latency
pub fn process_triton_message(resp: &SubscribeUpdate) {
    
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");

    if let Some(update) = &resp.update_oneof {
        match update {
            UpdateOneof::Transaction(tx_update) => {
                let start_time = Instant::now();

                if let Some(tx_info) = &tx_update.transaction {
                    if let Some(tx) = &tx_info.transaction {
                        let sig_bytes = tx.signatures.get(0).map(|s| s.clone());
                        let wallet_pubkey = get_wallet_keypair().pubkey();
                        let wallet_pubkey_bytes = wallet_pubkey.to_bytes();
                        let is_signer = if let Some(message) = &tx.message {
                            if let Some(header) = &message.header {
                                let num_signers = header.num_required_signatures as usize;
                                message.account_keys
                                    .iter()
                                    .take(num_signers)
                                    .any(|bytes| bytes.as_slice() == wallet_pubkey_bytes)
                            } else { false }
                        } else { false };
                        // Handoff to crossbeam worker for heavy processing
                        if let Some(ref sig_bytes) = sig_bytes {
                            log_event(EventType::GrpcDetectionProcessing, sig_bytes, start_time, None);
                        }
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
