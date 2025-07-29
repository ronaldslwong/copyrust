use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::build_tx::tx_builder::{build_and_sign_transaction, create_instruction};
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use crate::send_tx::rpc::send_tx_via_send_rpcs;
use crate::send_tx::zero_slot::send_tx_zeroslot;
use crate::send_tx::jito::send_jito_bundle;
use crate::send_tx::nextblock::send_tx_nextblock;
use crate::send_tx::block_razor::send_tx_blockrazor;
use chrono::Utc;
use std::time::Instant;
use rayon::prelude::*;

pub async fn send_rpc(cu_limit: u32, _cu_price: u64, mint: Pubkey, instructions: Vec<Instruction>) -> Result<String, Box<dyn std::error::Error>> {
    let rpc: &solana_client::rpc_client::RpcClient = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    
    let compute_budget_instruction = create_instruction(
        cu_limit,
        mint,
        instructions,
    );
    let tx = build_and_sign_transaction(
        rpc,
        &compute_budget_instruction,
        get_wallet_keypair(),
    )
    .ok();
    // println!("Signed tx, elapsed: {:.2?}", start_time.elapsed());
    let sig = send_tx_via_send_rpcs(&tx.unwrap()).await.unwrap();
    let now = Utc::now();
    println!(
        "[{}] - sell tx sent with sig: {}",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        sig
    );
    Ok(sig)
}

/// Send a transaction to a specific vendor
pub async fn send_to_vendor(vendor_name: &str, transaction: &Transaction) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let send_start = Instant::now();
    
    let result = match vendor_name {
        "rpc" => {
            send_tx_via_send_rpcs(transaction).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("RPC send failed: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
        }
        "zeroslot" => {
            send_tx_zeroslot(transaction).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("ZeroSlot send failed: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
        }
        "jito" => {
            send_jito_bundle(transaction).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Jito send failed: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
        }
        "nextblock" => {
            let config = crate::config_load::GLOBAL_CONFIG.get().expect("Config not initialized");
            send_tx_nextblock(transaction, &config.nextblock_api).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("NextBlock send failed: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
        }
        "blockrazor" => {
            let config = crate::config_load::GLOBAL_CONFIG.get().expect("Config not initialized");
            send_tx_blockrazor(transaction, &config.blockrazor_api, "fast", None, false).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("BlockRazor send failed: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
        }
        _ => {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unknown vendor: {}", vendor_name)
            )) as Box<dyn std::error::Error + Send + Sync>);
        }
    };
    
    // Individual vendor logging removed - now shown in comprehensive performance report
    
    result
}

/// Send all vendor transactions in parallel and return the first successful result
pub async fn send_all_vendors_parallel(
    vendor_transactions: &[(String, Transaction)],
    detection_time: Instant,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let send_start = Instant::now();
    
    println!(
        "[{}] - [GENERIC_SENDER] Starting parallel send to {} vendors",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        vendor_transactions.len()
    );
    
    // Create futures for all vendor sends with individual timing
    let mut futures = Vec::new();
    for (vendor_name, transaction) in vendor_transactions {
        let vendor_name = vendor_name.clone();
        let transaction = transaction.clone();
        let vendor_start = Instant::now();
        let future = async move {
            let result = send_to_vendor(&vendor_name, &transaction).await;
            let vendor_time = vendor_start.elapsed();
            (vendor_name, result, vendor_time)
        };
        futures.push(Box::pin(future));
    }
    
    // Wait for all vendors to complete and collect results
    let mut results = Vec::new();
    let mut successful_vendors = Vec::new();
    let mut failed_vendors = Vec::new();
    
    for future in futures {
        let (vendor_name, result, vendor_time) = future.await;
        
        match result {
            Ok(signature) => {
                successful_vendors.push((vendor_name.clone(), signature.clone(), vendor_time));
                results.push((vendor_name, Ok(signature), vendor_time));
            }
            Err(e) => {
                failed_vendors.push((vendor_name.clone(), vendor_time));
                results.push((vendor_name, Err(e), vendor_time));
            }
        }
    }
    
    // Display all vendor performance
    println!(
        "[{}] - [GENERIC_SENDER] ===== VENDOR PERFORMANCE REPORT =====",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f")
    );
    
    // Show successful vendors first (sorted by speed)
    if !successful_vendors.is_empty() {
        successful_vendors.sort_by(|a, b| a.2.cmp(&b.2)); // Sort by send time
        
        println!(
            "[{}] - [GENERIC_SENDER] ✅ SUCCESSFUL VENDORS ({}):",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            successful_vendors.len()
        );
        
        for (i, (vendor_name, signature, send_time)) in successful_vendors.iter().enumerate() {
            let rank = if i == 0 { "🥇" } else if i == 1 { "🥈" } else if i == 2 { "🥉" } else { "  " };
            let total_elapsed = send_start.duration_since(detection_time);
            
            println!(
                "[{}] - [GENERIC_SENDER] {} {}: {:.2?} | sig: {} | total elapsed: {:.2?}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                rank,
                vendor_name,
                send_time,
                signature,
                total_elapsed
            );
        }
    }
    
    // Show failed vendors
    if !failed_vendors.is_empty() {
        println!(
            "[{}] - [GENERIC_SENDER] ❌ FAILED VENDORS ({}):",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            failed_vendors.len()
        );
        
        for (vendor_name, send_time) in failed_vendors {
            println!(
                "[{}] - [GENERIC_SENDER]   {}: {:.2?} (FAILED)",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                vendor_name,
                send_time
            );
        }
    }
    
    // Show summary statistics
    let total_time = send_start.elapsed();
    let avg_time = if !results.is_empty() {
        let total: std::time::Duration = results.iter().map(|(_, _, time)| *time).sum();
        total / results.len() as u32
    } else {
        std::time::Duration::from_millis(0)
    };
    
    println!(
        "[{}] - [GENERIC_SENDER] 📊 SUMMARY: Total time: {:.2?} | Avg vendor time: {:.2?} | Success rate: {}/{}",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        total_time,
        avg_time,
        successful_vendors.len(),
        results.len()
    );
    
    println!(
        "[{}] - [GENERIC_SENDER] ================================================",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f")
    );
    
    // Return the fastest successful vendor
    if let Some((fastest_vendor, fastest_signature, _)) = successful_vendors.first() {
        Ok((fastest_vendor.clone(), fastest_signature.clone()))
    } else {
        // If we get here, all vendors failed
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "All vendors failed to send transaction"
        )) as Box<dyn std::error::Error + Send + Sync>)
    }
}

/// Legacy function for backward compatibility
pub async fn send_single_vendor(transaction: &Transaction, vendor_name: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    send_to_vendor(vendor_name, transaction).await
}

