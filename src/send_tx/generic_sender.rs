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
use crate::send_tx::flashblock::send_tx_flashblock;
use crate::send_tx::astralane::send_tx_astralane;
use crate::send_tx::temporal::send_tx_temporal;
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
        "flashblock" => {
            send_tx_flashblock(transaction).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Flashblock send failed: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
        }
        "astralane" => {
            send_tx_astralane(transaction).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Astralane send failed: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
        }
        "temporal" => {
            send_tx_temporal(transaction).await
                .map_err(|e| Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Temporal send failed: {}", e)
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
            #[cfg(feature = "verbose_logging")]
            println!(
                "[{}] - [GENERIC_SENDER] 🚀 Starting {} send...",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                vendor_name
            );
            let result = send_to_vendor(&vendor_name, &transaction).await;
            let vendor_time = vendor_start.elapsed();
            #[cfg(feature = "verbose_logging")]
            println!(
                "[{}] - [GENERIC_SENDER] ✅ {} completed in {:.2?}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                vendor_name,
                vendor_time
            );
            (vendor_name, result, vendor_time)
        };
        futures.push(Box::pin(future));
    }
    
    // Execute all futures in parallel using join_all
    #[cfg(feature = "verbose_logging")]
    println!(
        "[{}] - [GENERIC_SENDER] 🔄 Executing {} futures in parallel...",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        futures.len()
    );
    
    let parallel_start = Instant::now();
    let results = futures::future::join_all(futures).await;
    let parallel_time = parallel_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!(
        "[{}] - [GENERIC_SENDER] ✅ Parallel execution completed in {:.2?}",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        parallel_time
    );
    
    // Process results
    let mut successful_vendors = Vec::new();
    let mut failed_vendors = Vec::new();
    
    for (vendor_name, result, vendor_time) in results {
        match result {
            Ok(signature) => {
                successful_vendors.push((vendor_name.clone(), signature.clone(), vendor_time));
            }
            Err(e) => {
                failed_vendors.push((vendor_name.clone(), vendor_time));
                eprintln!("[GENERIC_SENDER] {} failed: {}", vendor_name, e);
            }
        }
    }
    
    // Display all vendor performance
    #[cfg(feature = "verbose_logging")]
    println!(
        "[{}] - [GENERIC_SENDER] ===== VENDOR PERFORMANCE REPORT =====",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f")
    );
    
    // Show successful vendors first (sorted by speed)
    if !successful_vendors.is_empty() {
        successful_vendors.sort_by(|a, b| a.2.cmp(&b.2)); // Sort by send time
        
        #[cfg(feature = "verbose_logging")]
        println!(
            "[{}] - [GENERIC_SENDER] ✅ SUCCESSFUL VENDORS ({}):",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            successful_vendors.len()
        );
        
        for (i, (vendor_name, signature, send_time)) in successful_vendors.iter().enumerate() {
            let rank = if i == 0 { "🥇" } else if i == 1 { "🥈" } else if i == 2 { "🥉" } else { "  " };
            let total_elapsed = send_start.duration_since(detection_time);
            
            #[cfg(feature = "verbose_logging")]
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
        #[cfg(feature = "verbose_logging")]
        println!(
            "[{}] - [GENERIC_SENDER] ❌ FAILED VENDORS ({}):",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            failed_vendors.len()
        );
        
        for (vendor_name, send_time) in failed_vendors {
            #[cfg(feature = "verbose_logging")]
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
    let avg_time = if !successful_vendors.is_empty() {
        let total: std::time::Duration = successful_vendors.iter().map(|(_, _, time)| *time).sum();
        total / successful_vendors.len() as u32
    } else {
        std::time::Duration::from_millis(0)
    };
    
    #[cfg(feature = "verbose_logging")]
    println!(
        "[{}] - [GENERIC_SENDER] 📊 SUMMARY: Total time: {:.2?} | Parallel execution: {:.2?} | Avg vendor time: {:.2?} | Success rate: {}/{}",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        total_time,
        parallel_time,
        avg_time,
        successful_vendors.len(),
        vendor_transactions.len()
    );
    
    #[cfg(feature = "verbose_logging")]
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

/// Test function to verify parallelism is working
pub async fn test_parallel_execution() {
    println!("[GENERIC_SENDER] 🧪 Testing parallel execution...");
    
    // Create dummy transactions for testing
    let dummy_transactions = vec![
        ("rpc".to_string(), Transaction::default()),
        ("zeroslot".to_string(), Transaction::default()),
        ("nextblock".to_string(), Transaction::default()),
        ("blockrazor".to_string(), Transaction::default()),
        ("astralane".to_string(), Transaction::default()),
        ("flashblock".to_string(), Transaction::default()),
    ];
    
    let test_start = Instant::now();
    
    // This will fail but we can see the timing
    let _result = send_all_vendors_parallel(&dummy_transactions, test_start).await;
    
    println!("[GENERIC_SENDER] 🧪 Parallel execution test completed");
}

