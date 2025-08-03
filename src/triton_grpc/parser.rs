use crate::config_load::GLOBAL_CONFIG;
use crate::geyser::{subscribe_update::UpdateOneof, SubscribeUpdate};
use crate::init::initialize::GLOBAL_RPC_CLIENT; // or wherever you defined it
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signer;
use crate::init::wallet_loader::get_wallet_keypair;
use std::time::Instant;
use crate::utils::logger::{log_event, EventType};
use crate::triton_grpc::crossbeam_worker::{ParsedTx, send_parsed_tx, is_signature_processed_by_feed};
use chrono::Utc;

// OPTIMIZATION: Enhanced parser for multiple feeds
pub fn process_triton_message(resp: &SubscribeUpdate, feed_id: &str) {
    let start_time = std::time::Instant::now();
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    
    // OPTIMIZATION: Log when message is received from GRPC stream
    #[cfg(feature = "verbose_logging")]
    {
        println!("[{}] - [TRITON] GRPC message received from {} (processing started)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), feed_id);
    }
    
    let update_check_start = std::time::Instant::now();
    let update_check = resp.update_oneof.is_some();
    let update_check_time = update_check_start.elapsed();
    
    if let Some(update) = &resp.update_oneof {
        match update {
            UpdateOneof::Transaction(tx_update) => {
                let tx_update_check_start = std::time::Instant::now();
                let tx_update_check = tx_update.transaction.is_some();
                let tx_update_check_time = tx_update_check_start.elapsed();
                
                if let Some(tx_info) = &tx_update.transaction {
                    let tx_info_check_start = std::time::Instant::now();
                    let tx_info_check = tx_info.transaction.is_some();
                    let tx_info_check_time = tx_info_check_start.elapsed();
                    
                    if let Some(tx) = &tx_info.transaction {
                        let sig_bytes = tx.signatures.get(0).map(|s| s.clone());
                        
                        // OPTIMIZATION: Extract signature string for deduplication
                        let sig_decode_start = std::time::Instant::now();
                        let sig_string = sig_bytes.as_ref()
                            .map(|s| bs58::encode(s).into_string())
                            .unwrap_or_default();
                        let sig_decode_time = sig_decode_start.elapsed();
                        
                        // OPTIMIZATION: Check if this signature was already processed by any feed
                        let dedup_start = std::time::Instant::now();
                        if is_signature_processed_by_feed(&sig_string, feed_id) {
                            // Skip processing - already handled by another feed
                            let dedup_time = dedup_start.elapsed();
                            #[cfg(feature = "verbose_logging")]
                            println!("[{}] - [TRITON] SKIPPED duplicate transaction for sig: {} (feed: {}) - already processed by another feed (dedup check took: {:.2?})", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, feed_id, dedup_time);
                            return;
                        }
                        let dedup_time = dedup_start.elapsed();
                        
                        // OPTIMIZATION: Log first detection of this transaction
                        println!("[{}] - [TRITON] FIRST DETECTION of transaction for sig: {} (feed: {}) (sig_decode: {:.2?}, dedup: {:.2?})", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, feed_id, sig_decode_time, dedup_time);
                        
                        let wallet_check_start = std::time::Instant::now();
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
                        let wallet_check_time = wallet_check_start.elapsed();
                        
                        // OPTIMIZATION: Only log if verbose mode is enabled
                        #[cfg(feature = "verbose_logging")]
                        if let Some(ref sig_bytes) = sig_bytes {
                            log_event(EventType::GrpcDetectionProcessing, sig_bytes, start_time, None);
                        }
                        
                        // Extract token balances from gRPC transaction data
                        // For now, we'll skip the conversion and use the existing RPC-based approach
                        // TODO: Implement proper conversion from proto TokenBalance to UiTransactionTokenBalance
                        let pre_token_balances = None;
                        let post_token_balances = None;
                        
                        let parsed = ParsedTx {
                            sig_bytes,
                            is_signer,
                            slot: Some(tx_update.slot),
                            detection_time: Some(start_time),
                            feed_id: feed_id.to_string(), // OPTIMIZATION: Track which feed detected this
                            pre_token_balances,
                            post_token_balances,
                        };
                        
                        let send_start = std::time::Instant::now();
                        send_parsed_tx(parsed);
                        let send_time = send_start.elapsed();
                        
                        let total_time = start_time.elapsed();
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[{}] - [TRITON] PROFILE - update_check: {:.2?}, tx_update_check: {:.2?}, tx_info_check: {:.2?}, sig_decode: {:.2?}, dedup: {:.2?}, wallet_check: {:.2?}, send: {:.2?}, total: {:.2?}", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), update_check_time, tx_update_check_time, tx_info_check_time, sig_decode_time, dedup_time, wallet_check_time, send_time, total_time);
                        }
                        return;
                    }
                }
            },
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

// OPTIMIZATION: Backward compatibility function
pub fn process_triton_message_legacy(resp: &SubscribeUpdate) {
    process_triton_message(resp, "triton_legacy")
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
